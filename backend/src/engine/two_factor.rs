//! Two-factor sign-in: an authenticator app's code after the password, and
//! one-time recovery codes for when the app is gone.

use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::user::User;
use crate::repo::{self, OrNotFound};
use crate::security::random::{random_bytes, token_digest};
use crate::security::totp;

/// How many recovery codes an account gets.
pub const RECOVERY_CODES: usize = 8;
const ISSUER: &str = "Nexc";

/// Whether an account signs in with a second factor.
#[derive(Debug, Serialize, ToSchema)]
pub struct TwoFactor {
    pub enabled: bool,
    /// Recovery codes that were not used yet.
    pub recovery_codes_left: i64,
}

/// What an authenticator app is set up with.
#[derive(Debug, Serialize, ToSchema)]
pub struct Setup {
    /// The secret, to type into the app when it cannot open the address.
    pub secret: String,
    /// The `otpauth://` address the app adds the account from.
    pub uri: String,
}

fn open(state: &AppState, user: Uuid, sealed: &[u8]) -> Result<Vec<u8>, AppError> {
    Ok(state.secret_box.open(sealed, user.as_bytes())?)
}

/// A recovery code as it is compared: without spaces or dashes, in lower case.
fn normal(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

pub async fn status(state: &AppState, user: Uuid) -> Result<TwoFactor, AppError> {
    let stored = repo::two_factor::find(&state.db, user)
        .await
        .or_not_found("user")?;
    Ok(TwoFactor {
        enabled: stored.enabled,
        recovery_codes_left: repo::two_factor::codes_left(&state.db, user).await?,
    })
}

/// Starts setting up: a new secret that is not in force until [`enable`]
/// proves the app has it. 409 while two-factor sign-in is on.
pub async fn setup(state: &AppState, user: &User) -> Result<Setup, AppError> {
    let stored = repo::two_factor::find(&state.db, user.id)
        .await
        .or_not_found("user")?;
    if stored.enabled {
        return Err(AppError::Conflict(
            "two-factor sign-in is already on; turn it off to set it up again".into(),
        ));
    }
    let secret = totp::generate_secret();
    let sealed = state.secret_box.seal(&secret, user.id.as_bytes())?;
    repo::two_factor::begin(&state.db, user.id, &sealed).await?;
    Ok(Setup {
        secret: totp::base32(&secret),
        uri: totp::uri(ISSUER, &user.email, &secret),
    })
}

/// Accepts `given` as the app's current code, once.
async fn app_code(state: &AppState, user: Uuid, given: &str) -> Result<bool, AppError> {
    let stored = repo::two_factor::find(&state.db, user)
        .await
        .or_not_found("user")?;
    let Some(sealed) = stored.secret else {
        return Ok(false);
    };
    let secret = open(state, user, &sealed)?;
    let now = chrono::Utc::now().timestamp();
    match totp::verify(&secret, given, now, stored.last_step) {
        Some(step) => Ok(repo::two_factor::accept_step(&state.db, user, step).await?),
        None => Ok(false),
    }
}

/// Turns two-factor sign-in on once `code` shows the app has the secret, and
/// returns the recovery codes, which are shown this once.
pub async fn enable(state: &AppState, user: Uuid, code: &str) -> Result<Vec<String>, AppError> {
    let stored = repo::two_factor::find(&state.db, user)
        .await
        .or_not_found("user")?;
    if stored.enabled {
        return Err(AppError::Conflict(
            "two-factor sign-in is already on".into(),
        ));
    }
    if stored.secret.is_none() {
        return Err(AppError::Conflict("start the setup first".into()));
    }
    if !app_code(state, user, code).await? {
        return Err(AppError::field("code", "that code is not right"));
    }
    let codes: Vec<String> = (0..RECOVERY_CODES)
        .map(|_| {
            let text = totp::base32(&random_bytes::<7>()).to_lowercase();
            format!("{}-{}", &text[..5], &text[5..10])
        })
        .collect();
    let hashes: Vec<String> = codes.iter().map(|c| token_digest(&normal(c))).collect();
    let mut tx = state.db.begin().await?;
    repo::two_factor::replace_codes(&mut tx, user, &hashes).await?;
    repo::two_factor::enable(&mut *tx, user).await?;
    tx.commit().await?;
    Ok(codes)
}

/// The second step of signing in. Nothing to do for an account without the
/// factor; otherwise `code` must be the app's current code or an unused
/// recovery code. A missing code and a wrong one are both a field error on
/// `code`, which is how a client learns to ask for it.
pub async fn check(state: &AppState, user: Uuid, code: Option<&str>) -> Result<(), AppError> {
    let stored = repo::two_factor::find(&state.db, user)
        .await
        .or_not_found("user")?;
    if !stored.enabled {
        return Ok(());
    }
    let Some(code) = code.map(str::trim).filter(|c| !c.is_empty()) else {
        return Err(AppError::field(
            "code",
            "enter the code from your authenticator app",
        ));
    };
    if app_code(state, user, code).await?
        || repo::two_factor::use_code(&state.db, user, &token_digest(&normal(code))).await?
    {
        Ok(())
    } else {
        Err(AppError::field("code", "that code is not right"))
    }
}

/// Turns two-factor sign-in off and forgets the secret and the recovery codes.
pub async fn clear(state: &AppState, user: Uuid) -> Result<(), AppError> {
    let mut tx = state.db.begin().await?;
    repo::two_factor::clear(&mut tx, user).await?;
    tx.commit().await?;
    Ok(())
}
