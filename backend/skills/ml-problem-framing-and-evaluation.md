---
id: ml-problem-framing-and-evaluation
name: ML framing and evaluation
category: ml
description: Frames a business problem as an ML task, sets baselines and metrics, designs leak-free validation (including time-series splits) and runs error analysis before any launch decision.
keywords: machine learning, problem framing, baseline, evaluation metric, precision, recall, auc, calibration, rmse, cross-validation, time series split, data leakage, error analysis, classification, regression, forecasting
source: Google - Rules of Machine Learning | https://developers.google.com/machine-learning/guides/rules-of-ml
source: scikit-learn - Cross-validation | https://scikit-learn.org/stable/modules/cross_validation.html
source: scikit-learn - Probability calibration | https://scikit-learn.org/stable/modules/calibration.html
source: Google Cloud - Professional Machine Learning Engineer | https://cloud.google.com/learn/certification/machine-learning-engineer
---
## When to use
Before building a model (is ML needed, what exactly is predicted), when choosing metrics and validation, and when judging whether a model is good enough to ship.

## Framing
1. Name the **decision** and the **action** the prediction drives, and the cost of each error type.
2. Define the **unit** (customer-week, transaction), the **prediction time**, and the **label** with its window ("churns within 30 days after prediction time").
3. List features available at prediction time only. Anything recorded later is leakage.
4. Start with a heuristic or simple model: Rules of ML #1 (launching without ML is fine) and #4 (keep the first model simple, get the pipeline right).
- Baselines: majority class / base rate; last value or seasonal naive for forecasts; logistic or linear regression. A model that cannot beat them is not ready.

## Metrics
- Precision = TP/(TP+FP); recall = TP/(TP+FN); F1 = 2PR/(P+R); F-beta weights recall beta times as much as precision.
- ROC AUC = probability a random positive scores above a random negative (threshold-free, insensitive to prevalence). Under heavy imbalance prefer PR AUC / average precision.
- Threshold: choose t minimising C_FP x FP(t) + C_FN x FN(t), or meeting a stated precision or recall floor.
- Probabilities: log loss; Brier = mean((p - y)^2); reliability curve; ECE = sum over bins (n_b/N) x |accuracy_b - confidence_b|. Calibrate on held-out data: sigmoid (Platt) for small sets, isotonic above roughly 1,000 samples, temperature scaling for multiclass.
- Regression: RMSE = sqrt(mean((y - yhat)^2)) punishes large errors; MAE is robust; MAPE is undefined at y = 0; MASE = MAE / in-sample MAE of the naive (seasonal) forecast, <1 beats naive.
- Report a confidence interval (bootstrap) and compare models on the same folds (paired test, e.g. McNemar). Quick check for a rate p measured on n cases: SE = sqrt(p(1-p)/n), 95% CI about p +/- 1.96 SE; recall 0.5 on 50 positives is +/- 0.14, too wide to rank close models.

## Validation
- Hold out a final test set and touch it once. Tune on train/validation or cross-validation; nested CV when tuning and estimating together.
- StratifiedKFold for imbalanced classes; GroupKFold when one entity has many rows (patients, customers).
- Time-ordered data: no shuffling; expanding or rolling-origin splits (TimeSeriesSplit) with a gap at least the label horizon; folds of equal duration; evaluate on periods after training (Rules of ML #33).
- Fit every preprocessing step inside a Pipeline per fold.

## Leakage checklist
- Target leakage: features created after the outcome (refund flag predicting fraud).
- Contamination: scaler, imputer, encoder or feature selection fit on all data.
- Duplicates or the same entity across splits.
- Non point-in-time joins (current customer status joined to historical rows).
- Suspicious signals: near-perfect validation scores, one feature dominating importance, big drop in production.

## Error analysis
Confusion matrix; metrics per slice (segment, region, device, new vs returning); read the highest-loss examples; label-noise check; compare against the baseline per slice; tie gains to the business metric.

## Output
Framing card (decision, unit, label, prediction time, costs); baseline and model table with metric, CI and slice results; validation design; leakage review; recommendation with risks.

## Pitfalls
Accuracy on imbalanced data; tuning on the test set; random splits on time series; quoting AUC when the decision needs a threshold; ignoring calibration when scores are used as probabilities.
