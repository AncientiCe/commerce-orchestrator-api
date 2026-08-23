//! The shipped Kubernetes manifests are part of the product: if they describe a
//! world the binary no longer runs in, "plug and deploy" is a lie. These tests
//! hold the manifests to the same contract `config.rs` enforces at startup.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_yaml::Value;

fn deploy_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("deploy")
}

fn manifest(name: &str) -> Value {
    let path = deploy_dir().join("kubernetes").join(name);
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e))
}

fn pod_spec(workload: &Value) -> &Value {
    &workload["spec"]["template"]["spec"]
}

fn container(workload: &Value, name: &str) -> Value {
    pod_spec(workload)["containers"]
        .as_sequence()
        .expect("containers")
        .iter()
        .find(|c| c["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("container {} not found", name))
        .clone()
}

fn env_from_names(container: &Value) -> Vec<String> {
    container["envFrom"]
        .as_sequence()
        .expect("envFrom")
        .iter()
        .filter_map(|source| {
            source["configMapRef"]["name"]
                .as_str()
                .or_else(|| source["secretRef"]["name"].as_str())
                .map(str::to_string)
        })
        .collect()
}

fn release_image() -> String {
    format!("orchestrator-api:{}", env!("CARGO_PKG_VERSION"))
}

#[test]
fn deployment_runs_the_current_release_image() {
    let deployment = manifest("deployment.yaml");
    let image = container(&deployment, "orchestrator-server")["image"]
        .as_str()
        .expect("image")
        .to_string();
    assert_eq!(
        image,
        release_image(),
        "deployment image tag must track the workspace version"
    );
}

#[test]
fn deployment_reads_state_from_postgres_not_a_mounted_volume() {
    let deployment = manifest("deployment.yaml");
    let server = container(&deployment, "orchestrator-server");

    for volume in pod_spec(&deployment)["volumes"]
        .as_sequence()
        .into_iter()
        .flatten()
    {
        assert!(
            volume["persistentVolumeClaim"].is_null(),
            "production state lives in Postgres; no PVC should back the pod"
        );
    }
    for mount in server["volumeMounts"].as_sequence().into_iter().flatten() {
        assert_ne!(
            mount["mountPath"].as_str(),
            Some("/data"),
            "/data was the file-backed persistence path and is no longer read"
        );
    }
    assert!(
        !deploy_dir().join("kubernetes").join("pvc.yaml").exists(),
        "pvc.yaml describes the retired file-backed persistence contract"
    );

    let sources = env_from_names(&server);
    assert!(sources.contains(&"orchestrator-api".to_string()));
    assert!(sources.contains(&"orchestrator-api-secret".to_string()));
}

#[test]
fn config_and_secret_carry_the_postgres_contract() {
    let configmap = manifest("configmap.yaml");
    let data = configmap["data"].as_mapping().expect("configmap data");
    assert!(
        !data.contains_key(Value::from("PERSISTENCE_PATH")),
        "PERSISTENCE_PATH is ignored in production and misleads operators"
    );

    let secret = manifest("secret.yaml");
    let secret_data = secret["stringData"].as_mapping().expect("secret data");
    assert!(
        secret_data.contains_key(Value::from("DATABASE_URL")),
        "DATABASE_URL is required in production and carries credentials"
    );
    for required in ["UCP_SIGNING_KEY_ID", "UCP_SIGNING_KEY"] {
        assert!(
            secret_data.contains_key(Value::from(required)),
            "{} is required in production since v0.9.0",
            required
        );
    }
}

#[test]
fn deployment_and_autoscaler_allow_multiple_replicas() {
    let deployment = manifest("deployment.yaml");
    let replicas = deployment["spec"]["replicas"].as_u64().expect("replicas");
    assert!(
        replicas >= 2,
        "Postgres persistence and SKIP LOCKED dequeue make multi-replica safe; got {}",
        replicas
    );

    let hpa = manifest("hpa.yaml");
    let min_replicas = hpa["spec"]["minReplicas"].as_u64().expect("minReplicas");
    assert!(
        min_replicas >= 2,
        "autoscaler must not scale below the deployment's baseline; got {}",
        min_replicas
    );

    let pdb = manifest("pdb.yaml");
    assert!(
        pdb["spec"]["minAvailable"].as_u64().expect("minAvailable") >= 2,
        "a single available replica during disruption defeats multi-replica availability"
    );
}

#[test]
fn migration_job_runs_the_release_image_in_migrate_only_mode() {
    let job = manifest("job-migrate.yaml");
    assert_eq!(job["kind"].as_str(), Some("Job"));

    let migrate = container(&job, "migrate");
    assert_eq!(
        migrate["image"].as_str().expect("image"),
        release_image(),
        "migrations must run the same binary as the servers they migrate for"
    );

    let args: Vec<&str> = migrate["args"]
        .as_sequence()
        .expect("args")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        args.contains(&"--migrate"),
        "the job must run the migrate-only entrypoint, got {:?}",
        args
    );

    assert_eq!(
        pod_spec(&job)["restartPolicy"].as_str(),
        Some("Never"),
        "a failed migration should surface, not restart in place"
    );
    assert!(
        job["spec"]["backoffLimit"].as_u64().is_some(),
        "bound the retries so a broken migration fails the rollout"
    );

    let sources = env_from_names(&migrate);
    assert!(
        sources.contains(&"orchestrator-api-secret".to_string()),
        "the job needs DATABASE_URL from the same secret as the deployment"
    );
}

#[test]
fn service_monitor_scrapes_the_orchestrator_metrics_endpoint() {
    let monitor = manifest("servicemonitor.yaml");
    assert_eq!(monitor["kind"].as_str(), Some("ServiceMonitor"));
    assert_eq!(
        monitor["spec"]["selector"]["matchLabels"]["app"].as_str(),
        Some("orchestrator-api"),
        "the monitor must select the orchestrator Service"
    );

    let endpoint = monitor["spec"]["endpoints"]
        .as_sequence()
        .expect("endpoints")
        .first()
        .expect("at least one endpoint")
        .clone();
    assert_eq!(endpoint["port"].as_str(), Some("http"));
    assert_eq!(
        endpoint["path"].as_str(),
        Some("/metrics"),
        "metrics are exposed on the same port as the API"
    );
}

#[test]
fn grafana_dashboard_only_charts_metrics_the_orchestrator_actually_exports() {
    let path = deploy_dir()
        .join("grafana")
        .join("orchestrator-dashboard.json");
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let dashboard: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));

    let panels = dashboard["panels"].as_array().expect("panels");
    assert!(
        !panels.is_empty(),
        "a dashboard with no panels helps nobody"
    );

    seed_every_metric_family();
    let exported = exported_sample_names();
    let charted = charted_metric_names(&dashboard);
    assert!(
        !charted.is_empty(),
        "dashboard panels must contain PromQL targets"
    );

    for name in &charted {
        assert!(
            exported.contains(name),
            "dashboard charts {} which /metrics never exposes; exported: {:?}",
            name,
            exported
        );
    }
    for required in [
        "orchestrator_queue_depth",
        "orchestrator_provider_circuit_state",
    ] {
        assert!(
            charted.contains(required),
            "the starter dashboard must chart {}",
            required
        );
    }
}

/// Touch every metric family so the exposition lists them all.
fn seed_every_metric_family() {
    orchestrator_observability::incr("deploy_manifest_test");
    orchestrator_observability::observe_provider_http_call("GET", "catalog", "200", 0.01);
    orchestrator_observability::observe_operation("get_cart", "ok", 0.01);
    orchestrator_observability::set_provider_circuit_state("catalog", 0);
    orchestrator_observability::set_queue_depth("outbox", 0);
    orchestrator_observability::observe_delivery_attempts("outbox_delivery", "ok", 1);
}

fn exported_sample_names() -> BTreeSet<String> {
    let text = orchestrator_observability::render_prometheus().expect("render metrics");
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| line.split(['{', ' ']).next())
        .map(|name| {
            name.strip_suffix("_bucket")
                .or_else(|| name.strip_suffix("_sum"))
                .or_else(|| name.strip_suffix("_count"))
                .unwrap_or(name)
                .to_string()
        })
        .collect()
}

fn charted_metric_names(dashboard: &serde_json::Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for panel in dashboard["panels"].as_array().into_iter().flatten() {
        for target in panel["targets"].as_array().into_iter().flatten() {
            let Some(expr) = target["expr"].as_str() else {
                continue;
            };
            for token in expr.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                if let Some(metric) = token.strip_prefix("orchestrator_") {
                    let full = format!("orchestrator_{}", metric);
                    names.insert(
                        full.strip_suffix("_bucket")
                            .or_else(|| full.strip_suffix("_sum"))
                            .or_else(|| full.strip_suffix("_count"))
                            .unwrap_or(&full)
                            .to_string(),
                    );
                }
            }
        }
    }
    names
}
