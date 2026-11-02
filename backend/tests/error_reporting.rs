//! What a reporter walking an error's causes may print: every cause, and
//! none of the secrets, row values or customer data a cause can carry.
#![forbid(unsafe_code)]

use anyhow::Context as _;
use nexc::domain::error_chain;
use nexc::repo;
use sqlx::PgPool;
use sqlx::postgres::PgDatabaseError;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

const ROW_VALUE: &str = "dana@customer.example";

/// A unique violation: the server's `DETAIL` quotes the row.
async fn duplicate(pool: &PgPool) -> sqlx::Error {
    sqlx::query("INSERT INTO probe (email) VALUES ($1)")
        .bind(ROW_VALUE)
        .execute(pool)
        .await
        .unwrap_err()
}

#[sqlx::test(migrations = false)]
async fn a_row_value_in_a_server_detail_stays_out(pool: PgPool) {
    sqlx::query("CREATE TABLE probe (email text PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO probe (email) VALUES ($1)")
        .bind(ROW_VALUE)
        .execute(&pool)
        .await
        .unwrap();

    // The server did send it, so the assertions below prove something.
    let err = duplicate(&pool).await;
    let detail = err
        .as_database_error()
        .and_then(|db| db.try_downcast_ref::<PgDatabaseError>())
        .and_then(PgDatabaseError::detail)
        .unwrap();
    assert!(detail.contains(ROW_VALUE), "{detail}");

    let with_context = || async {
        Err::<(), _>(duplicate(&pool).await)
            .context("copying users failed")
            .unwrap_err()
    };
    let internal = nexc::domain::AppError::Internal(with_context().await);
    for report in [
        error_chain(&err),
        error_chain(with_context().await.as_ref()),
        error_chain(&internal),
    ] {
        assert!(report.contains("duplicate key value"), "{report}");
        assert!(!report.contains(ROW_VALUE), "{report}");
    }
    // sqlx's message quotes its source: the walk does not say it twice.
    assert_eq!(
        error_chain(with_context().await.as_ref())
            .matches("duplicate key")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_connection_string_stays_out() {
    let parsed = repo::connect("postgres://nexc:hunter2@db.example.com:notaport/x", 1)
        .await
        .unwrap_err();
    let report = error_chain(parsed.context("cannot connect").as_ref());
    assert!(report.starts_with("cannot connect: "), "{report}");
    assert!(!report.contains("hunter2"), "{report}");
}

#[tokio::test]
async fn a_url_a_client_quotes_loses_its_credentials_and_query() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let client = reqwest::Client::new();
    let refused = client
        .get(format!(
            "http://user:hunter2@127.0.0.1:{port}/v1?api_key=hunter3#hunter4"
        ))
        .send()
        .await
        .unwrap_err();
    let plain = refused.to_string();
    assert!(plain.contains("hunter3"), "{plain}");
    let report = error_chain(&refused);
    assert!(report.contains(&format!("127.0.0.1:{port}/v1")), "{report}");
    assert!(report.contains("onnection refused"), "{report}");
    for secret in ["hunter2", "hunter3", "hunter4"] {
        assert!(!report.contains(secret), "{report}");
    }
}

#[tokio::test]
async fn a_body_a_client_could_not_decode_stays_out() {
    const DOCUMENT: &str = r#""the text of a customer's contract""#;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = socket.read(&mut request).await.unwrap();
        let answer = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{DOCUMENT}",
            DOCUMENT.len()
        );
        socket.write_all(answer.as_bytes()).await.unwrap();
    });
    // As the document parser's call: the file's name goes in the query.
    let response = reqwest::get(format!("http://{host}/parse?filename=contract.pdf"))
        .await
        .unwrap();
    let refused = response.json::<u32>().await.unwrap_err();
    server.await.unwrap();

    assert_eq!(
        error_chain(&refused),
        format!(
            "error decoding response body for url (http://{host}/parse): \
             invalid type: string at line 1 column 35"
        )
    );
}
