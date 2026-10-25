---
id: ml-mlops-lifecycle
name: MLOps lifecycle
category: ml
description: Takes models from notebook to production with reproducible training, feature stores, a model registry, CI/CD/CT pipelines and monitoring for drift and performance, assessed against Google's MLOps maturity levels.
keywords: mlops, model deployment, model registry, feature store, continuous training, ci/cd, model monitoring, data drift, concept drift, training-serving skew, reproducibility, mlflow, vertex ai, sagemaker, canary, rollback
source: Google Cloud - MLOps: continuous delivery and automation pipelines in machine learning | https://docs.cloud.google.com/architecture/mlops-continuous-delivery-and-automation-pipelines-in-machine-learning
source: MLflow - Model Registry | https://mlflow.org/docs/latest/ml/model-registry/
source: Google - Rules of Machine Learning | https://developers.google.com/machine-learning/guides/rules-of-ml
source: AWS - Certified Machine Learning Engineer - Associate | https://aws.amazon.com/certification/certified-machine-learning-engineer-associate/
---
## When to use
Planning or reviewing how a model is trained, versioned, deployed, monitored and retrained; diagnosing production decay; assessing an ML platform.

## Maturity levels (Google Cloud MLOps guide)
| Level | Traits | Next step |
|---|---|---|
| 0 manual | notebooks, hand-off of a model file, rare releases, no prediction tracking | automate the training pipeline |
| 1 pipeline automation | orchestrated pipeline retrains on fresh data (continuous training), data and model validation, same pipeline in dev and prod, metadata store | automate pipeline CI/CD |
| 2 CI/CD automation | pipeline code built, tested and deployed automatically; registry, metadata store, orchestrator | keep data/model analysis reviews |

## Reproducibility
Pin and log for every run: code commit, data snapshot or table version, environment (container digest, lockfile), hyperparameters, random seeds, metrics, artifacts. Track runs in an experiment tracker (e.g. MLflow); a model nobody can retrain is a liability.

## Features
- Feature store: shared definitions, offline store for training, online store for serving, point-in-time-correct joins for training sets.
- Prevent training-serving skew: one transformation code path for both, or log serving features and train on them (Rules of ML #29, #32).

## Registry and release
- Register each candidate as a version with signature, lineage (run, data, code) and evaluation report; use aliases (e.g. champion, challenger) so serving points at an alias and rollback is an alias switch.
- Gates before promotion: beats current champion on the agreed metric and on key slices; latency and memory within budget; data and schema checks pass; fairness and policy checks pass; approver recorded.
- Rollout: shadow (score without acting), canary (small traffic share, automatic rollback on SLO breach), A/B test for business impact, sized before launch: n per arm about 16 x sigma^2 / delta^2 for 80% power at two-sided alpha 0.05 (sigma^2 = p(1-p) for a conversion rate p, delta = smallest lift worth detecting); batch scoring compares against previous outputs.

## CI / CD / CT
- CI: unit tests for feature code, data validation (schema, ranges, missingness), training smoke test on a sample, model tests against thresholds.
- CD: deploy the pipeline and the serving model through environments with infrastructure as code.
- CT triggers: schedule, new labelled data, drift alarm, performance drop. Schema skew stops the pipeline; value drift may trigger retraining.

## Monitoring
- Input drift per feature: PSI = sum over bins (a_i - e_i) x ln(a_i / e_i), with a_i, e_i the actual and expected bin shares. Common rule of thumb: below 0.1 stable, 0.1-0.25 watch, above 0.25 investigate. Also KS test or Jensen-Shannon distance.
- Prediction drift (score distribution, positive rate), concept drift (relation changes; seen when labels arrive), delayed-label proxies.
- Performance on labelled feedback per slice; operational SLOs (p95 latency, error rate, throughput, cost per 1,000 predictions); feature freshness and null rates.
- Every alert has an owner and a runbook: retrain, roll back, or fix upstream data.

## Output
Lifecycle design: maturity assessment with gaps; pipeline diagram; registry and promotion policy; monitoring table (signal, metric, threshold, action, owner); rollback and retirement plan.

## Pitfalls
Retraining on a schedule without checking the new model beats the old; monitoring only infrastructure; drift alarms with no action; features computed differently online and offline; unversioned training data.
