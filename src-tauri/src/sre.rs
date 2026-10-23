use crate::{agent, liara::Liara, models::*, security::*, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

pub fn tr(language: &str, en: &str, fa: &str) -> String {
    if language == "fa" { fa } else { en }.into()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub platform: String,
    pub provider_id: String,
    pub url: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub id: String,
    pub name: String,
    pub project_id: Option<String>,
    pub run_id: Option<String>,
    pub services: Vec<Service>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub interval_seconds: u64,
    pub slow_ms: u64,
    pub health_path: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            interval_seconds: 30,
            slow_ms: 2000,
            health_path: "/".into(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if !(15..=300).contains(&self.interval_seconds)
            || !(500..=30000).contains(&self.slow_ms)
            || !self.health_path.starts_with('/')
            || self.health_path.starts_with("//")
            || self.health_path.contains(['?', '#', '\\', '\r', '\n'])
            || self.health_path.len() > 200
        {
            return Err("Invalid monitoring settings.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub at: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub service_id: String,
    pub provider_state: String,
    pub http_status: Option<u16>,
    pub latency_ms: Option<u64>,
    pub url: Option<String>,
    pub cpu_cores: Option<f64>,
    pub memory_bytes: Option<f64>,
    pub cpu_limit: Option<f64>,
    pub memory_limit_bytes: Option<f64>,
    pub metrics_at: Option<i64>,
    pub logs: Vec<LogLine>,
    #[serde(default)]
    pub logs_available: bool,
    pub limitations: Vec<String>,
    pub collection_failed: bool,
    pub auth_failed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    pub id: String,
    pub service_id: String,
    pub kind: String,
    pub severity: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub id: String,
    pub at: String,
    pub observations: Vec<Observation>,
    pub signals: Vec<Signal>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hypothesis {
    pub title: String,
    pub confidence: String,
    pub explanation: String,
    pub evidence_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub kind: String,
    pub service_id: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub summary: String,
    pub hypotheses: Vec<Hypothesis>,
    pub recommendations: Vec<Recommendation>,
    pub limitations: Vec<String>,
    pub source: String,
    pub at: String,
    pub sample_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub id: String,
    pub key: String,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub signals: Vec<Signal>,
    pub diagnosis: Option<Diagnosis>,
    pub good_checks: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub incident_id: String,
    pub service_id: String,
    pub kind: String,
    pub status: String,
    pub at: String,
    pub finished_at: Option<String>,
    pub message: String,
    pub from_plan_id: String,
    pub target_plan_id: Option<String>,
    pub release_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: String,
    pub account_id: String,
    pub target: Target,
    pub settings: Settings,
    pub language: String,
    pub status: String,
    pub started_at: String,
    pub updated_at: String,
    pub samples: Vec<Sample>,
    pub incidents: Vec<Incident>,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub latest_diagnosis: Option<Diagnosis>,
    pub analyzing: bool,
    pub analysis_error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    pub monitor_id: String,
    pub incident_id: String,
    pub service_id: String,
    pub kind: String,
    pub description: String,
    pub from_monthly_price: Option<f64>,
    pub monthly_price: Option<f64>,
    pub target_plan_id: Option<String>,
    pub expires_at: String,
    pub account_id: String,
    pub provider_id: String,
    pub from_plan_id: String,
    pub bundle_id: String,
    pub release_id: Option<String>,
}
struct Job {
    stop: AtomicBool,
    wake: Notify,
    cancel: Notify,
    cancel_analysis: Notify,
}
struct Runtime {
    jobs: BTreeMap<String, Arc<Job>>,
    approvals: BTreeMap<String, Approval>,
}
pub struct Supervisor {
    pub store: Arc<Store>,
    runtime: Mutex<Runtime>,
    pub action_active: AtomicBool,
    pub notify: Arc<dyn Fn(&Monitor) + Send + Sync>,
}
impl Supervisor {
    pub fn new(store: Arc<Store>, notify: Arc<dyn Fn(&Monitor) + Send + Sync>) -> Self {
        Self {
            store,
            runtime: Mutex::new(Runtime {
                jobs: BTreeMap::new(),
                approvals: BTreeMap::new(),
            }),
            action_active: AtomicBool::new(false),
            notify,
        }
    }
    pub fn active(&self) -> bool {
        self.runtime
            .lock()
            .map(|r| !r.jobs.is_empty())
            .unwrap_or(true)
            || self.action_active.load(Ordering::SeqCst)
    }
    pub fn get(&self, id: &str, account: &str) -> Result<Monitor> {
        let m: Monitor = self.store.get("sre", id)?;
        if m.account_id != account {
            return Err("Monitoring session belongs to another account.".into());
        }
        Ok(m)
    }
    fn save(&self, m: &mut Monitor) -> Result<()> {
        m.updated_at = now();
        self.store.save("sre", &m.id, &m.account_id, m)?;
        (self.notify)(m);
        Ok(())
    }
    pub fn list(&self, account: &str) -> Result<Vec<Monitor>> {
        let mut monitors: Vec<Monitor> = self.store.list("sre", account)?;
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| "Monitoring state unavailable")?;
        for m in &mut monitors {
            if m.status == "running" && !runtime.jobs.contains_key(&m.id) {
                m.status = "interrupted".into();
                m.analyzing = false;
                self.store.save("sre", &m.id, account, m)?;
            }
        }
        Ok(monitors)
    }
    pub fn start(
        self: &Arc<Self>,
        api: Liara,
        account: &str,
        target: Target,
        settings: Settings,
        language: String,
    ) -> Result<Monitor> {
        settings.validate()?;
        if target.services.is_empty() || target.services.len() > 16 {
            return Err("Select a project containing 1–16 supported services.".into());
        }
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "Monitoring state unavailable")?;
        for id in runtime.jobs.keys() {
            let m = self.get(id, account)?;
            if m.target.id == target.id {
                return Ok(m);
            }
        }
        if runtime.jobs.len() >= 4 {
            return Err(
                "Stop an existing session before starting more than four concurrent monitors."
                    .into(),
            );
        }
        let mut m = Monitor {
            id: id(),
            account_id: account.into(),
            target,
            settings,
            language,
            status: "running".into(),
            started_at: now(),
            updated_at: now(),
            samples: vec![],
            incidents: vec![],
            actions: vec![],
            latest_diagnosis: None,
            analyzing: false,
            analysis_error: None,
        };
        let job = Arc::new(Job {
            stop: AtomicBool::new(false),
            wake: Notify::new(),
            cancel: Notify::new(),
            cancel_analysis: Notify::new(),
        });
        self.save(&mut m)?;
        runtime.jobs.insert(m.id.clone(), job.clone());
        let supervisor = self.clone();
        let monitor_id = m.id.clone();
        let account = account.to_string();
        tokio::spawn(async move {
            loop {
                if job.stop.load(Ordering::SeqCst) {
                    break;
                }
                let current = match supervisor.get(&monitor_id, &account) {
                    Ok(m) => m,
                    Err(_) => break,
                };
                let since = current.samples.last().map(|s| s.at.as_str());
                let collection = collect(
                    &api,
                    &current.target,
                    &current.settings,
                    &current.language,
                    since,
                );
                let result = tokio::select! {_ = job.cancel.notified()=>break,result=tokio::time::timeout(Duration::from_secs(45),collection)=>result};
                if job.stop.load(Ordering::SeqCst) {
                    break;
                }
                let sample = match result {
                    Ok(Ok(sample)) => sample,
                    Ok(Err(error)) => {
                        let mut sample = failed_sample(&current);
                        for observation in &mut sample.observations {
                            observation.auth_failed = error.starts_with("HTTP 401:");
                            observation
                                .limitations
                                .push(redact(&error, &[api.secret()]));
                        }
                        sample
                    }
                    Err(_) => failed_sample(&current),
                };
                let auth = sample.observations.iter().any(|o| o.auth_failed);
                let updated = {
                    let _lock = match supervisor.runtime.lock() {
                        Ok(lock) => lock,
                        Err(_) => break,
                    };
                    if job.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let mut updated = match supervisor.get(&monitor_id, &account) {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    if updated.status != "running" {
                        break;
                    }
                    record_sample(&mut updated, sample);
                    if auth {
                        updated.status = "needs_connection".into();
                    }
                    if supervisor.save(&mut updated).is_err() {
                        break;
                    }
                    updated
                };
                let needs_analysis = needs_analysis(&updated);
                if needs_analysis {
                    supervisor.spawn_analysis(api.clone(), &updated);
                }
                if auth {
                    break;
                }
                tokio::select! {_ = job.cancel.notified()=>break,_ = job.wake.notified()=>{},_ = tokio::time::sleep(Duration::from_secs(updated.settings.interval_seconds))=>{}}
            }
            if let Ok(mut runtime) = supervisor.runtime.lock() {
                runtime.jobs.remove(&monitor_id);
            }
        });
        Ok(m)
    }
    pub fn stop(&self, id: &str, account: &str) -> Result<Monitor> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "Monitoring state unavailable")?;
        let mut m = self.get(id, account)?;
        if let Some(job) = runtime.jobs.remove(id) {
            job.stop.store(true, Ordering::SeqCst);
            job.cancel.notify_one();
            job.cancel_analysis.notify_waiters();
        }
        runtime.approvals.retain(|_, a| a.monitor_id != id);
        m.status = "stopped".into();
        m.analyzing = false;
        self.save(&mut m)?;
        Ok(m)
    }
    pub fn check_now(&self, id: &str, account: &str) -> Result<Monitor> {
        let m = self.get(id, account)?;
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| "Monitoring state unavailable")?;
        let job = runtime
            .jobs
            .get(id)
            .ok_or("Start monitoring before requesting another check.")?;
        job.wake.notify_one();
        Ok(m)
    }
    pub fn analyze(self: &Arc<Self>, api: Liara, id: &str, account: &str) -> Result<Monitor> {
        let m = self.get(id, account)?;
        if m.samples.is_empty() {
            return Err("Wait for the first health check.".into());
        }
        self.spawn_analysis(api, &m);
        self.get(id, account)
    }
    fn spawn_analysis(self: &Arc<Self>, _api: Liara, m: &Monitor) {
        let _lock = match self.runtime.lock() {
            Ok(lock) => lock,
            Err(_) => return,
        };
        let mut current = match self.get(&m.id, &m.account_id) {
            Ok(current) => current,
            Err(_) => return,
        };
        if current.analyzing || current.status != "running" {
            return;
        }
        let Some(job) = _lock.jobs.get(&m.id).cloned() else {
            return;
        };
        current.analyzing = true;
        current.analysis_error = None;
        let _ = self.save(&mut current);
        let supervisor = self.clone();
        let input = current.clone();
        tokio::spawn(async move {
            let cancelled = job.cancel_analysis.notified();
            tokio::pin!(cancelled);
            cancelled.as_mut().enable();
            if job.stop.load(Ordering::SeqCst) {
                return;
            }
            let result = tokio::select! {_=&mut cancelled=>return,result=agent::diagnose_sre(&input)=>result};
            let _lock = match supervisor.runtime.lock() {
                Ok(lock) => lock,
                Err(_) => return,
            };
            let mut current = match supervisor.get(&input.id, &input.account_id) {
                Ok(m) => m,
                Err(_) => return,
            };
            if current.status != "running" {
                return;
            }
            current.analyzing = false;
            match result {
                Ok(diagnosis) => {
                    if let Some(incident) = current.incidents.iter_mut().find(|i| {
                        input.incidents.last().is_some_and(|old| old.id == i.id)
                            && i.closed_at.is_none()
                    }) {
                        incident.diagnosis = Some(diagnosis.clone());
                    }
                    current.latest_diagnosis = Some(diagnosis);
                }
                Err(error) => {
                    current.analysis_error = Some(redact(&error, &[]));
                }
            }
            let _ = supervisor.save(&mut current);
        });
    }
    pub fn associate_run(&self, run: &DeploymentRun) {
        if run.status != "succeeded" {
            return;
        }
        let Ok(_lock) = self.runtime.lock() else {
            return;
        };
        let Ok(monitors) = self.store.list::<Monitor>("sre", &run.plan.account_id) else {
            return;
        };
        for mut m in monitors {
            if m.target.project_id.as_deref() == Some(&run.plan.project_id)
                && m.target.services.iter().all(|s| {
                    run.resources
                        .iter()
                        .any(|r| r.resource_id.as_deref() == Some(&s.provider_id))
                })
            {
                m.target.run_id = Some(run.id.clone());
                let _ = self.save(&mut m);
            }
        }
    }
    pub fn put_approval(&self, approval: Approval) -> Result<Approval> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "Monitoring state unavailable")?;
        runtime
            .approvals
            .retain(|_, a| a.monitor_id != approval.monitor_id);
        runtime
            .approvals
            .insert(approval.id.clone(), approval.clone());
        Ok(approval)
    }
    pub fn consume_approval(&self, id: &str, account: &str) -> Result<Approval> {
        let a = self
            .runtime
            .lock()
            .map_err(|_| "Monitoring state unavailable")?
            .approvals
            .remove(id)
            .ok_or("Approval is missing, expired or already used.")?;
        let m = self.get(&a.monitor_id, account)?;
        validate_approval(&a, &m, account)?;
        Ok(a)
    }
    pub fn update_action(
        &self,
        account: &str,
        monitor_id: &str,
        action: Action,
    ) -> Result<Monitor> {
        let _lock = self
            .runtime
            .lock()
            .map_err(|_| "Monitoring state unavailable")?;
        let mut m = self.get(monitor_id, account)?;
        if let Some(existing) = m.actions.iter_mut().find(|a| a.id == action.id) {
            *existing = action;
        } else {
            m.actions.push(action);
        }
        if m.actions.len() > 30 {
            m.actions.remove(0);
        }
        self.save(&mut m)?;
        Ok(m)
    }
}

fn needs_analysis(m: &Monitor) -> bool {
    m.samples.last().is_some_and(|sample| {
        m.incidents.last().is_some_and(|i| {
            i.closed_at.is_none()
                && i.diagnosis
                    .as_ref()
                    .is_none_or(|d| d.source == "rules" && d.sample_id == sample.id)
        })
    })
}
fn choose_upgrade<'a>(
    plans: &'a [PlanOption],
    current: &PlanOption,
    platform: &str,
    disk: f64,
    memory: bool,
    cpu: bool,
) -> Option<&'a PlanOption> {
    plans
        .iter()
        .filter(|p| {
            p.category == current.category
                && p.bundle_id == current.bundle_id
                && p.region == current.region
                && p.cpu >= current.cpu
                && p.ram >= current.ram
                && p.disk >= disk
                && p.monthly_price.is_some()
                && (!memory || p.ram > current.ram)
                && (!cpu || p.cpu > current.cpu)
                && (p.supports.is_empty() || p.supports.iter().any(|s| s == platform))
        })
        .min_by(|a, b| {
            a.monthly_price
                .unwrap()
                .total_cmp(&b.monthly_price.unwrap())
        })
}

pub async fn targets(api: &Liara, store: &Store, account: &str) -> Result<Vec<Target>> {
    let (apps, dbs) = tokio::join!(api.get("/v1/projects"), api.get("/v1/databases"));
    let apps = apps?;
    let dbs = dbs?;
    let apps = apps["projects"]
        .as_array()
        .ok_or("Application list format is unsupported.")?;
    let dbs = dbs["databases"]
        .as_array()
        .ok_or("Database list format is unsupported.")?;
    let runs: Vec<DeploymentRun> = store.list("run", account)?;
    let mut seen = BTreeSet::new();
    let mut covered = BTreeSet::new();
    let mut result = vec![];
    for run in runs {
        if seen.contains(&run.plan.project_id) {
            continue;
        }
        let mut services = vec![];
        for service in &run.plan.services {
            let Some(resource) = run
                .resources
                .iter()
                .find(|r| r.service_id == service.id)
                .and_then(|r| r.resource_id.as_deref())
            else {
                continue;
            };
            let inventory = if service.kind == "app" {
                apps
            } else if service.kind == "database" {
                dbs
            } else {
                continue;
            };
            let found = inventory.iter().any(|item| {
                ["_id", "DBId", "id"]
                    .iter()
                    .any(|key| item[key].as_str() == Some(resource))
            });
            if !found {
                continue;
            }
            covered.insert(resource.to_string());
            services.push(Service {
                id: service.id.clone(),
                name: service.name.clone(),
                kind: service.kind.clone(),
                platform: service.platform.clone(),
                provider_id: resource.into(),
                url: None,
            });
        }
        if services.iter().any(|s| s.kind == "app") {
            seen.insert(run.plan.project_id.clone());
            let name = store
                .get::<ProjectAnalysis>("project", &run.plan.project_id)
                .map(|p| p.name)
                .unwrap_or_else(|_| run.plan.services[0].name.clone());
            result.push(Target {
                id: format!("project:{}", run.plan.project_id),
                name,
                project_id: Some(run.plan.project_id),
                run_id: Some(run.id),
                services,
            });
        }
    }
    for app in apps {
        let (Some(id), Some(name), Some(platform)) = (
            app["_id"].as_str(),
            app["project_id"].as_str(),
            app["type"].as_str(),
        ) else {
            continue;
        };
        if covered.contains(id) {
            continue;
        }
        result.push(Target {
            id: format!("app:{id}"),
            name: name.into(),
            project_id: None,
            run_id: None,
            services: vec![Service {
                id: id.into(),
                name: name.into(),
                kind: "app".into(),
                platform: platform.into(),
                provider_id: id.into(),
                url: None,
            }],
        });
    }
    Ok(result)
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|n| n.is_finite() && *n >= 0.0)
}
pub fn metric_summary(value: &Value, key: &str) -> (Option<f64>, Option<i64>) {
    let Some(items) = value[key].as_array() else {
        return (None, None);
    };
    if items.is_empty() {
        return (None, None);
    }
    let mut sum = 0.0;
    let mut at = None;
    for item in items {
        let Some(n) = number(&item["value"][1]) else {
            return (None, None);
        };
        let Some(t) = item["value"][0]
            .as_i64()
            .or_else(|| item["value"][0].as_f64().map(|n| n as i64))
        else {
            return (None, None);
        };
        sum += n;
        at = Some(at.map_or(t, |old: i64| old.min(t)));
    }
    if at.is_some_and(|t| {
        chrono::Utc::now().timestamp() - t > 180 || t > chrono::Utc::now().timestamp() + 30
    }) {
        (None, at)
    } else {
        (Some(sum), at)
    }
}
fn bounded_logs(value: &Value, secrets: &[String], since: i64) -> Vec<LogLine> {
    let mut lines = vec![];
    if let Some(items) = value.as_array() {
        for item in items {
            if let (Some(message), Some(at)) = (item["message"].as_str(), item["datetime"].as_str())
            {
                if chrono::DateTime::parse_from_rfc3339(at).is_ok_and(|t| t.timestamp() >= since) {
                    lines.push(LogLine {
                        at: at.into(),
                        message: redact(message, secrets).chars().take(800).collect(),
                    });
                }
            }
        }
    }
    // Official CLI v2 log groups; supported if the provider returns that shape.
    if let Some(groups) = value["data"].as_array() {
        for group in groups {
            if let Some(values) = group["values"].as_array() {
                for line in values {
                    if let (Some(at), Some(message)) = (line[0].as_str(), line[1].as_str()) {
                        if let Ok(nanos) = at.parse::<i64>() {
                            let seconds = nanos / 1_000_000_000;
                            if seconds >= since {
                                if let Some(date) = chrono::DateTime::from_timestamp(seconds, 0) {
                                    lines.push(LogLine {
                                        at: date.to_rfc3339(),
                                        message: redact(message, secrets)
                                            .chars()
                                            .take(800)
                                            .collect(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    lines.sort_by(|a, b| a.at.cmp(&b.at));
    lines.dedup_by(|a, b| a.at == b.at && a.message == b.message);
    if lines.len() > 30 {
        lines.drain(0..lines.len() - 30);
    }
    lines
}
fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            let a = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_multicast()
                && a[0] != 0
                && a[0] < 224
                && !(a[0] == 100 && (64..=127).contains(&a[1]))
                && !(a[0] == 198 && (18..=19).contains(&a[1]))
        }
        std::net::IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return public_ip(v4.into());
            }
            !v.is_loopback()
                && !v.is_unspecified()
                && !v.is_multicast()
                && (v.segments()[0] & 0xfe00) != 0xfc00
                && (v.segments()[0] & 0xffc0) != 0xfe80
        }
    }
}
pub async fn probe(url: &str) -> Result<(u16, u64)> {
    let parsed = url::Url::parse(url).map_err(|_| "Invalid project health URL.")?;
    let host = parsed.host_str().ok_or("Health URL has no hostname.")?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
    {
        return Err("Only public HTTPS project URLs are supported.".into());
    }
    let started = Instant::now();
    let addresses: Vec<_> = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|_| "Project hostname could not be resolved.")?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|a| !public_ip(a.ip())) {
        return Err("The health URL must resolve to public IP addresses.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(parsed.clone())
        .send()
        .await
        .map_err(|_| "The website did not respond, or its TLS/connection failed.")?;
    Ok((
        response.status().as_u16(),
        started.elapsed().as_millis() as u64,
    ))
}
pub async fn collect(
    api: &Liara,
    target: &Target,
    settings: &Settings,
    language: &str,
    since: Option<&str>,
) -> Result<Sample> {
    let since = since
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.timestamp())
        .unwrap_or_else(|| chrono::Utc::now().timestamp() - 300);
    let mut jobs = tokio::task::JoinSet::new();
    let permits = Arc::new(tokio::sync::Semaphore::new(4));
    let me = api.get("/v1/me").await?;
    let plans = crate::liara::normalize_plans(&me["plans"]);
    for service in target.services.clone() {
        let api = api.clone();
        let settings = settings.clone();
        let language = language.to_string();
        let plans = plans.clone();
        let permits = permits.clone();
        jobs.spawn(async move {
            let _permit = permits.acquire_owned().await.unwrap();
            observe(&api, &service, &plans, &settings, &language, since).await
        });
    }
    let mut observations = vec![];
    while let Some(observation) = jobs.join_next().await {
        observations.push(observation.map_err(|_| "A monitoring check was interrupted.")?);
    }
    Ok(Sample {
        id: id(),
        at: now(),
        observations,
        signals: vec![],
    })
}
async fn observe(
    api: &Liara,
    service: &Service,
    plans: &[PlanOption],
    settings: &Settings,
    language: &str,
    since: i64,
) -> Observation {
    let base = if service.kind == "app" {
        format!("/v1/projects/{}", service.name)
    } else {
        format!("/v1/databases/{}", service.provider_id)
    };
    let key = if service.kind == "app" {
        "project"
    } else {
        "database"
    };
    let mut observation = Observation {
        service_id: service.id.clone(),
        provider_state: "unknown".into(),
        http_status: None,
        latency_ms: None,
        url: None,
        cpu_cores: None,
        memory_bytes: None,
        cpu_limit: None,
        memory_limit_bytes: None,
        metrics_at: None,
        logs: vec![],
        logs_available: false,
        limitations: vec![],
        collection_failed: false,
        auth_failed: false,
    };
    let detail = match api.get(&base).await {
        Ok(v) => v,
        Err(error) => {
            observation.auth_failed = error.starts_with("HTTP 401:");
            observation.collection_failed = true;
            observation
                .limitations
                .push(redact(&error, &[api.secret()]));
            return observation;
        }
    };
    let d = &detail[key];
    let actual = d["_id"].as_str().or_else(|| d["id"].as_str());
    if actual != Some(service.provider_id.as_str()) || d["type"].as_str() != Some(&service.platform)
    {
        observation.collection_failed = true;
        observation.limitations.push(tr(
            language,
            "Resource identity changed; no action is allowed.",
            "شناسه منبع تغییر کرده است؛ هیچ اقدامی مجاز نیست.",
        ));
        return observation;
    }
    observation.provider_state = d["status"].as_str().unwrap_or("unknown").into();
    if d["scale"].as_u64() == Some(0) {
        observation.provider_state = "STOPPED".into();
    }
    let mut secrets = vec![api.secret()];
    if let Some(envs) = d["envs"].as_array() {
        secrets.extend(
            envs.iter()
                .filter_map(|v| v["value"].as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        );
    }
    if let Some(envs) = d["envs"].as_object() {
        secrets.extend(
            envs.values()
                .filter_map(|v| v.as_str().or_else(|| v["value"].as_str()))
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        );
    }
    if let Some(password) = d["root_password"].as_str() {
        secrets.push(password.into());
    }
    let nested: Vec<String> = secrets
        .iter()
        .filter_map(|value| url::Url::parse(value).ok())
        .filter_map(|url| {
            url.password().map(|password| {
                percent_encoding::percent_decode_str(password)
                    .decode_utf8_lossy()
                    .into_owned()
            })
        })
        .filter(|value| !value.is_empty())
        .collect();
    secrets.extend(nested);
    let plan = plans.iter().find(|p| {
        p.category == service.kind
            && d["planID"].as_str() == Some(&p.id)
            && d["bundlePlanID"].as_str().is_none_or(|b| b == p.bundle_id)
    });
    if let Some(plan) = plan {
        let replicas = d["scale"].as_u64().unwrap_or(1).max(1) as f64;
        observation.cpu_limit = Some(plan.cpu * replicas);
        observation.memory_limit_bytes = Some(plan.ram * replicas * 1024.0 * 1024.0 * 1024.0);
    }
    let summary_path = format!("{base}/metrics/summary");
    let summary = api.get(&summary_path);
    let logs = async {
        if service.kind == "app" {
            api.get(&format!("{base}/logs?since={since}")).await
        } else {
            Err(tr(language,"Database logs are not exposed by the verified API; database status/metrics are checked.","لاگ پایگاه داده در API تاییدشده در دسترس نیست؛ وضعیت و مصرف منابع بررسی می‌شود."))
        }
    };
    let health = async {
        if service.kind != "app" {
            return None;
        }
        let suffix = if d["network"].is_null() {
            "iran.liara.run"
        } else {
            "liara.run"
        };
        let mut host = format!("https://{}.{suffix}", service.name);
        if d["defaultSubdomain"].as_bool() == Some(false) {
            match api
                .get(&format!("/v1/domains?project={}", service.name))
                .await
            {
                Ok(v) => {
                    if let Some(domain) = v["domains"]
                        .as_array()
                        .and_then(|items| {
                            items.iter().find(|item| {
                                item["certificatesStatus"].as_str() == Some("ACTIVE")
                                    && item["project"]["_id"].as_str() == Some(&service.provider_id)
                            })
                        })
                        .and_then(|item| item["name"].as_str())
                    {
                        host = format!("https://{domain}");
                    } else {
                        return Some((
                            host,
                            Err("No active HTTPS domain could be verified for this app.".into()),
                        ));
                    }
                }
                Err(error) => return Some((host, Err(error))),
            }
        }
        let url = format!("{host}{}", settings.health_path);
        let result = probe(&url).await;
        Some((url, result))
    };
    let (summary, logs, health) = tokio::join!(summary, logs, health);
    match summary {
        Ok(value) => {
            let (cpu, at) = metric_summary(&value, "cpuUsage");
            let (memory, at_mem) = metric_summary(&value, "memoryUsage");
            observation.cpu_cores = cpu;
            observation.memory_bytes = memory;
            observation.metrics_at = match (at, at_mem) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            if cpu.is_none() || memory.is_none() {
                observation.limitations.push(tr(language,"Resource metrics are unavailable, empty or stale; they are not treated as zero.","داده مصرف منابع موجود نیست، خالی است یا قدیمی شده؛ این وضعیت مصرف صفر محسوب نمی‌شود."));
            }
        }
        Err(error) => {
            observation.auth_failed |= error.starts_with("HTTP 401:");
            observation.limitations.push(redact(&error, &secrets));
        }
    }
    match logs {
        Ok(logs) => {
            observation.logs_available = logs.is_array() || logs["data"].is_array();
            observation.logs = bounded_logs(&logs, &secrets, since);
            if !observation.logs_available {
                observation.limitations.push(tr(
                    language,
                    "The provider returned an unsupported log format.",
                    "قالب لاگ ارائه‌شده توسط لیارا پشتیبانی نمی‌شود.",
                ));
            }
        }
        Err(error) => observation.limitations.push(redact(&error, &secrets)),
    }
    if let Some((url, result)) = health {
        observation.url = Some(url);
        match result {
            Ok((status, latency)) => {
                observation.http_status = Some(status);
                observation.latency_ms = Some(latency);
            }
            Err(error) => {
                observation.collection_failed = true;
                observation.limitations.push(
                    tr(
                        language,
                        "Website health check failed:",
                        "بررسی سلامت وب‌سایت ناموفق بود:",
                    ) + " "
                        + &redact(&error, &secrets),
                );
            }
        }
    }
    observation
}
fn failed_sample(m: &Monitor) -> Sample {
    Sample{id:id(),at:now(),observations:m.target.services.iter().map(|s|Observation{service_id:s.id.clone(),provider_state:"unknown".into(),http_status:None,latency_ms:None,url:None,cpu_cores:None,memory_bytes:None,cpu_limit:None,memory_limit_bytes:None,metrics_at:None,logs:vec![],logs_available:false,limitations:vec![tr(&m.language,"The check timed out or the provider could not be reached. This is not evidence that the website itself is down.","زمان بررسی تمام شد یا اتصال به لیارا برقرار نشد. این وضعیت به تنهایی نشان نمی‌دهد وب‌سایت قطع است.")],collection_failed:true,auth_failed:false}).collect(),signals:vec![]}
}
pub fn signals(
    sample: &Sample,
    previous: Option<&Sample>,
    settings: &Settings,
    language: &str,
) -> Vec<Signal> {
    let mut result = vec![];
    for o in &sample.observations {
        let before = previous.and_then(|s| {
            s.observations
                .iter()
                .find(|old| old.service_id == o.service_id)
        });
        let mut add = |kind: &str, severity: &str, en: &str, fa: &str| {
            result.push(Signal {
                id: format!("{}:{kind}", o.service_id),
                service_id: o.service_id.clone(),
                kind: kind.into(),
                severity: severity.into(),
                message: tr(language, en, fa),
            })
        };
        if o.collection_failed {
            add(
                "collection",
                "warning",
                "Some checks could not be completed. Review connectivity and API access.",
                "بعضی بررسی‌ها کامل نشدند. اتصال و دسترسی API باید بررسی شوند.",
            );
        }
        if o.http_status.is_some_and(|s| s >= 500) {
            add(
                "http_5xx",
                "critical",
                "The website returned a server error.",
                "وب‌سایت خطای سرور برگرداند.",
            );
        } else if o.http_status.is_some_and(|s| s >= 400) {
            add(
                "http_4xx",
                "warning",
                "The checked page is unavailable or requires permission.",
                "صفحه بررسی‌شده در دسترس نیست یا به مجوز نیاز دارد.",
            );
        }
        if o.latency_ms.is_some_and(|n| n > settings.slow_ms)
            && before.is_some_and(|old| old.latency_ms.is_some_and(|n| n > settings.slow_ms))
        {
            add(
                "slow",
                "warning",
                "The website was slow in two consecutive checks.",
                "وب‌سایت در دو بررسی پیاپی کند پاسخ داد.",
            );
        }
        if !matches!(
            o.provider_state.as_str(),
            "ACTIVE" | "RUNNING" | "READY" | "running" | "ready" | "unknown"
        ) {
            add(
                "provider_state",
                "critical",
                "Liara reports that this service is stopped or unhealthy.",
                "لیارا این سرویس را متوقف یا ناسالم گزارش می‌کند.",
            );
        }
        let high_memory = |x: &Observation| {
            x.memory_bytes
                .zip(x.memory_limit_bytes)
                .is_some_and(|(used, total)| total > 0.0 && used / total >= 0.85)
        };
        if high_memory(o)
            && before.is_some_and(|old| {
                high_memory(old)
                    && o.metrics_at.is_some()
                    && old.metrics_at.is_some()
                    && old.metrics_at != o.metrics_at
            })
        {
            add(
                "memory",
                "warning",
                "Memory usage remained above 85% of the advertised plan capacity.",
                "مصرف حافظه در دو بررسی بالاتر از ۸۵ درصد ظرفیت اعلام‌شده پلن بود.",
            );
        }
        let high_cpu = |x: &Observation| {
            x.cpu_cores
                .zip(x.cpu_limit)
                .is_some_and(|(used, total)| total > 0.0 && used / total >= 0.85)
        };
        if high_cpu(o)
            && before.is_some_and(|old| {
                high_cpu(old)
                    && o.metrics_at.is_some()
                    && old.metrics_at.is_some()
                    && old.metrics_at != o.metrics_at
            })
        {
            add(
                "cpu",
                "warning",
                "CPU usage remained high in consecutive checks.",
                "مصرف پردازنده در دو بررسی پیاپی بالا بود.",
            );
        }
        if o.logs.iter().any(|line| {
            [
                "traceback",
                "exception",
                "out of memory",
                "oomkilled",
                "fatal",
                "error:",
            ]
            .iter()
            .any(|word| line.message.to_lowercase().contains(word))
        }) {
            add(
                "logs",
                "warning",
                "Recent application logs contain errors that need investigation.",
                "در لاگ‌های اخیر برنامه خطاهایی دیده می‌شود که به بررسی نیاز دارند.",
            );
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}
pub fn fallback(sample: &Sample, language: &str) -> Diagnosis {
    let hypotheses=sample.signals.iter().map(|s|Hypothesis{title:s.message.clone(),confidence:if s.kind=="http_5xx"||s.kind=="provider_state"{"high"}else{"medium"}.into(),explanation:tr(language,"This is an observation, not a proven root cause. Compare logs, resources and recent changes before taking action.","این مورد یک مشاهده است و علت قطعی محسوب نمی‌شود. پیش از اقدام، لاگ‌ها، مصرف منابع و تغییرات اخیر را مقایسه کنید."),evidence_ids:vec![s.id.clone()]}).collect();
    let mut recommendations = vec![];
    for signal in &sample.signals {
        let kind = match signal.kind.as_str() {
            "cpu" | "memory" => "resize",
            "http_5xx" | "provider_state" => "restart",
            "logs" => "code_fix",
            _ => "investigate",
        };
        if !recommendations
            .iter()
            .any(|r: &Recommendation| r.kind == kind && r.service_id == signal.service_id)
        {
            recommendations.push(Recommendation {
                kind: kind.into(),
                service_id: signal.service_id.clone(),
                reason: signal.message.clone(),
            });
        }
    }
    Diagnosis{summary:tr(language,"We found evidence that needs attention. The details below distinguish observations from possible causes.","شواهدی پیدا شد که به بررسی نیاز دارد. در جزئیات، مشاهدات از علت‌های احتمالی جدا شده‌اند."),hypotheses,recommendations,limitations:vec![tr(language,"Rules-based findings are available while AI analysis is pending or unavailable. CPU rates and memory bytes follow the observed Liara metric format; empty or stale values remain unknown.","تا آماده شدن تحلیل هوش مصنوعی یا در نبود آن، گزارش مبتنی بر قواعد نمایش داده می‌شود. واحدهای مصرف از قالب مشاهده‌شده API لیارا گرفته شده‌اند؛ داده خالی یا قدیمی نامشخص می‌ماند.")],source:"rules".into(),at:now(),sample_id:sample.id.clone()}
}
pub fn symptom_cleared(signal: &Signal, sample: &Sample, settings: &Settings) -> bool {
    let Some(o) = sample
        .observations
        .iter()
        .find(|o| o.service_id == signal.service_id)
    else {
        return false;
    };
    if o.collection_failed {
        return false;
    }
    match signal.kind.as_str() {
        "collection" => true,
        "http_5xx" | "http_4xx" => o
            .http_status
            .is_some_and(|status| (200..400).contains(&status)),
        "slow" => o.latency_ms.is_some_and(|n| n <= settings.slow_ms),
        "memory" => o
            .memory_bytes
            .zip(o.memory_limit_bytes)
            .is_some_and(|(used, total)| total > 0.0 && used / total < 0.85),
        "cpu" => o
            .cpu_cores
            .zip(o.cpu_limit)
            .is_some_and(|(used, total)| total > 0.0 && used / total < 0.85),
        "provider_state" => matches!(
            o.provider_state.as_str(),
            "ACTIVE" | "RUNNING" | "READY" | "running" | "ready"
        ),
        "logs" => o.logs_available && !sample.signals.iter().any(|s| s.id == signal.id),
        _ => false,
    }
}
pub fn record_sample(m: &mut Monitor, mut sample: Sample) {
    sample.signals = signals(&sample, m.samples.last(), &m.settings, &m.language);
    for incident in m.incidents.iter_mut().filter(|i| i.closed_at.is_none()) {
        if !incident
            .signals
            .iter()
            .all(|signal| symptom_cleared(signal, &sample, &m.settings))
        {
            incident.good_checks = 0;
        } else {
            incident.good_checks += 1;
            if incident.good_checks >= 2 {
                incident.closed_at = Some(sample.at.clone());
            }
        }
    }
    let fresh: Vec<_> = sample
        .signals
        .iter()
        .filter(|signal| {
            !m.incidents
                .iter()
                .any(|i| i.closed_at.is_none() && i.signals.iter().any(|s| s.id == signal.id))
        })
        .cloned()
        .collect();
    if !fresh.is_empty() {
        m.latest_diagnosis = Some(fallback(&sample, &m.language));
        let key = fresh
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>()
            .join("|");
        m.incidents.push(Incident {
            id: id(),
            key,
            opened_at: sample.at.clone(),
            closed_at: None,
            signals: fresh,
            diagnosis: Some(fallback(&sample, &m.language)),
            good_checks: 0,
        });
    }
    m.samples.push(sample);
    if m.samples.len() > 60 {
        m.samples.remove(0);
    }
    if m.incidents.len() > 40 {
        m.incidents.remove(0);
    }
}
pub fn validate_approval(a: &Approval, m: &Monitor, account: &str) -> Result<()> {
    if a.account_id != account
        || m.account_id != account
        || m.status != "running"
        || a.expires_at < now()
        || !m
            .incidents
            .iter()
            .any(|i| i.id == a.incident_id && i.closed_at.is_none())
        || !m
            .target
            .services
            .iter()
            .any(|s| s.id == a.service_id && s.provider_id == a.provider_id)
    {
        return Err("Approval expired, the incident resolved, or the project/account changed. Review a new action.".into());
    }
    if !m
        .samples
        .last()
        .is_some_and(|sample| sample.signals.iter().any(|s| s.service_id == a.service_id))
    {
        return Err(
            "Symptoms changed after approval. Run a fresh check and review the action again."
                .into(),
        );
    }
    if m.actions
        .iter()
        .any(|a| matches!(a.status.as_str(), "submitting" | "uncertain" | "verifying"))
    {
        return Err("A previous corrective action has an unfinished or uncertain result. Reconcile it before retrying.".into());
    }
    Ok(())
}

pub async fn prepare_action(
    api: &Liara,
    supervisor: &Supervisor,
    account: &Account,
    monitor_id: &str,
    service_id: &str,
    kind: &str,
) -> Result<Approval> {
    if !matches!(kind, "restart" | "resize") {
        return Err("This action must use the reviewed source-fix workflow.".into());
    }
    let m = supervisor.get(monitor_id, &account.id)?;
    if m.status != "running" {
        return Err("Start monitoring before reviewing a corrective action.".into());
    }
    let service = m
        .target
        .services
        .iter()
        .find(|s| s.id == service_id)
        .ok_or("Unknown monitored service")?;
    if kind == "restart" && service.kind != "app" {
        return Err(tr(&m.language,"Database restart is not available through this verified workflow. Use Liara console.","راه‌اندازی مجدد پایگاه داده در این گردش تاییدشده در دسترس نیست. از کنسول لیارا استفاده کنید."));
    }
    let latest = m.samples.last().ok_or("Wait for the first check")?;
    if chrono::DateTime::parse_from_rfc3339(&latest.at).map_or(true, |at| {
        chrono::Utc::now().timestamp() - at.timestamp() > 180
    }) || !latest.signals.iter().any(|s| s.service_id == service_id)
    {
        return Err(tr(
            &m.language,
            "Current evidence no longer supports this action. Run another check.",
            "شواهد فعلی دیگر این اقدام را پشتیبانی نمی‌کنند. یک بررسی تازه اجرا کنید.",
        ));
    }
    let incident = m
        .incidents
        .iter()
        .rev()
        .find(|i| i.closed_at.is_none() && i.signals.iter().any(|s| s.service_id == service_id))
        .ok_or("This service has no open incident. Run a new check.")?;
    for saved in supervisor.list(&account.id)? {
        if saved.actions.iter().any(|a| {
            matches!(a.status.as_str(), "submitting" | "verifying" | "uncertain")
                && saved
                    .target
                    .services
                    .iter()
                    .any(|s| s.id == a.service_id && s.provider_id == service.provider_id)
        }) {
            return Err(tr(&m.language,"A previous action has an uncertain or unfinished result. Reconcile its result before another action.","نتیجه اقدام قبلی هنوز روشن یا کامل نیست. پیش از اقدام دیگر، نتیجه قبلی را بررسی کنید."));
        }
    }
    let base = if service.kind == "app" {
        format!("/v1/projects/{}", service.name)
    } else {
        format!("/v1/databases/{}", service.provider_id)
    };
    let key = if service.kind == "app" {
        "project"
    } else {
        "database"
    };
    let detail = api.get(&base).await?;
    let resource = &detail[key];
    if resource["_id"].as_str() != Some(&service.provider_id)
        || resource["type"].as_str() != Some(&service.platform)
    {
        return Err("Resource identity or platform changed. Select the project again.".into());
    }
    let from = resource["planID"]
        .as_str()
        .ok_or("Current plan unavailable")?;
    let bundle = resource["bundlePlanID"]
        .as_str()
        .ok_or("Current feature bundle unavailable")?;
    let plans = api.get("/v1/me").await?;
    let plans = crate::liara::normalize_plans(&plans["plans"]);
    let current = plans
        .iter()
        .find(|p| p.category == service.kind && p.id == from && p.bundle_id == bundle);
    let replicas = if service.kind == "app" {
        resource["scale"].as_u64().unwrap_or(1).max(1) as f64
    } else {
        1.0
    };
    let mut target = None;
    let mut target_price = current
        .and_then(|p| p.monthly_price)
        .map(|cost| cost * replicas);
    if kind == "resize" {
        let latest = m
            .samples
            .last()
            .ok_or("Wait for a current resource check")?;
        let memory = latest
            .signals
            .iter()
            .any(|s| s.service_id == service.id && s.kind == "memory");
        let cpu = latest
            .signals
            .iter()
            .any(|s| s.service_id == service.id && s.kind == "cpu");
        if !memory && !cpu {
            return Err(tr(&m.language,"An upgrade needs sustained CPU or memory evidence; latency alone is insufficient.","پیشنهاد ارتقا به شواهد مصرف مداوم پردازنده یا حافظه نیاز دارد؛ کندی به تنهایی کافی نیست."));
        }
        let current = current.ok_or("Current plan pricing/capacity could not be verified.")?;
        let observation = latest
            .observations
            .iter()
            .find(|o| o.service_id == service.id)
            .ok_or("Wait for current resource measurements")?;
        if observation
            .metrics_at
            .is_none_or(|at| chrono::Utc::now().timestamp() - at > 180)
            || observation.cpu_limit != Some(current.cpu * replicas)
            || observation.memory_limit_bytes
                != Some(current.ram * replicas * 1024.0 * 1024.0 * 1024.0)
        {
            return Err(tr(
                &m.language,
                "The plan or measurements changed. Run another check before reviewing an upgrade.",
                "پلن یا داده مصرف تغییر کرده است. پیش از بررسی ارتقا، یک بررسی تازه اجرا کنید.",
            ));
        }

        let disk = if service.kind == "app" {
            let disks = api
                .get(&format!("/v1/projects/{}/disks", service.provider_id))
                .await?;
            crate::storage::required_storage(
                &[],
                disks["disks"]
                    .as_array()
                    .ok_or("Disk allocation unavailable")?,
            )?
        } else {
            number(&resource["volumeSize"]).ok_or("Database disk allocation unavailable")?
        };
        let next=choose_upgrade(&plans,current,&service.platform,disk,memory,cpu).ok_or_else(||tr(&m.language,"No compatible upgrade with verified pricing was found. Existing resources remain unchanged.","پلن ارتقای سازگار با قیمت تاییدشده پیدا نشد. منابع موجود تغییر نکردند."))?;
        target = Some(next.id.clone());
        target_price = next.monthly_price.map(|cost| cost * replicas);
    }
    let release = if service.kind == "app" {
        let releases = api.get(&format!("{base}/releases?page=1&count=1")).await?;
        releases["currentRelease"].as_str().map(str::to_string)
    } else {
        None
    };
    let description = if kind == "restart" {
        tr(&m.language,&format!("Restart {} once. This may briefly interrupt visitors. Then check its health and the original symptoms; a successful API call is not proof the problem is fixed.",service.name),&format!("سرویس {} یک بار دوباره راه‌اندازی شود. ممکن است دسترسی کاربران کوتاه‌مدت قطع شود. سپس سلامت و نشانه‌های اولیه بررسی می‌شوند؛ موفقیت درخواست API به معنی رفع مشکل نیست.",service.name))
    } else {
        tr(&m.language,&format!("Upgrade {} from {} to {}. Keep its existing data, compute capacity, region and feature bundle. Review the new monthly estimate; then verify the original symptoms.",service.name,from,target.as_deref().unwrap()),&format!("پلن {} از {} به {} ارتقا یابد. داده‌ها، حداقل توان پردازشی، موقعیت و بسته امکانات حفظ می‌شوند. هزینه ماهانه جدید را بررسی کنید؛ سپس نشانه‌های اولیه دوباره بررسی می‌شوند.",service.name,from,target.as_deref().unwrap()))
    };
    supervisor.put_approval(Approval {
        id: id(),
        monitor_id: m.id,
        incident_id: incident.id.clone(),
        service_id: service.id.clone(),
        kind: kind.into(),
        description,
        from_monthly_price: current
            .and_then(|p| p.monthly_price)
            .map(|cost| cost * replicas),
        monthly_price: target_price,
        target_plan_id: target,
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(15)).to_rfc3339(),
        account_id: account.id.clone(),
        provider_id: service.provider_id.clone(),
        from_plan_id: from.into(),
        bundle_id: bundle.into(),
        release_id: release,
    })
}

pub async fn apply_action(
    api: &Liara,
    supervisor: &Supervisor,
    approval: Approval,
) -> Result<Monitor> {
    let m = supervisor.get(&approval.monitor_id, &approval.account_id)?;
    validate_approval(&approval, &m, &approval.account_id)?;
    let service = m
        .target
        .services
        .iter()
        .find(|s| s.id == approval.service_id)
        .ok_or("Unknown service")?;
    let base = if service.kind == "app" {
        format!("/v1/projects/{}", service.name)
    } else {
        format!("/v1/databases/{}", service.provider_id)
    };
    let key = if service.kind == "app" {
        "project"
    } else {
        "database"
    };
    let detail = api.get(&base).await?;
    let d = &detail[key];
    if d["_id"].as_str() != Some(&approval.provider_id)
        || d["planID"].as_str() != Some(&approval.from_plan_id)
        || d["bundlePlanID"].as_str() != Some(&approval.bundle_id)
    {
        return Err(tr(
            &m.language,
            "Resource or plan changed after approval. Review it again.",
            "منبع یا پلن پس از تایید تغییر کرده است. دوباره بررسی کنید.",
        ));
    }
    if service.kind == "app" {
        let releases = api.get(&format!("{base}/releases?page=1&count=1")).await?;
        if releases["currentRelease"].as_str() != approval.release_id.as_deref() {
            return Err(
                "A release changed after approval. Run a new check and review again.".into(),
            );
        }
    }
    let mut body = json!({});
    let path = if approval.kind == "restart" {
        format!("{base}/actions/restart")
    } else {
        let target = approval
            .target_plan_id
            .as_ref()
            .ok_or("Approved target plan is missing")?;
        let me = api.get("/v1/me").await?;
        let plans = crate::liara::normalize_plans(&me["plans"]);
        let p = plans
            .iter()
            .find(|p| {
                p.category == service.kind && p.id == *target && p.bundle_id == approval.bundle_id
            })
            .ok_or("The approved plan is no longer available.")?;
        let replicas = if service.kind == "app" {
            d["scale"].as_u64().unwrap_or(1).max(1) as f64
        } else {
            1.0
        };
        if p.monthly_price.map(|cost| cost * replicas) != approval.monthly_price {
            return Err(tr(
                &m.language,
                "Pricing changed. Review a fresh upgrade before continuing.",
                "قیمت تغییر کرده است. پیش از ادامه، پیشنهاد تازه را بررسی کنید.",
            ));
        }
        if service.kind == "app" {
            let disks = api
                .get(&format!("/v1/projects/{}/disks", service.provider_id))
                .await?;
            if crate::storage::required_storage(
                &[],
                disks["disks"].as_array().ok_or("Disk data unavailable")?,
            )? > p.disk
            {
                return Err("Disk allocations changed; review the upgrade again.".into());
            }
        }
        body = json!({"planID":target});
        if service.kind == "database" {
            let disk = number(&d["volumeSize"]).ok_or("Database disk size unavailable")?;
            if disk > p.disk {
                return Err("Database disk allocation changed; review the upgrade again.".into());
            }
            body["disk"] = json!(disk);
        }
        format!("{base}/resize")
    };
    validate_approval(
        &approval,
        &supervisor.get(&approval.monitor_id, &approval.account_id)?,
        &approval.account_id,
    )?;
    let mut action = Action {
        id: approval.id.clone(),
        incident_id: approval.incident_id.clone(),
        service_id: approval.service_id.clone(),
        kind: approval.kind.clone(),
        status: "submitting".into(),
        at: now(),
        finished_at: None,
        message: approval.description.clone(),
        from_plan_id: approval.from_plan_id.clone(),
        target_plan_id: approval.target_plan_id.clone(),
        release_id: approval.release_id.clone(),
    };
    supervisor.update_action(&approval.account_id, &approval.monitor_id, action.clone())?;
    if let Err(error) = api.post(&path, body).await {
        action.status = if [
            "HTTP 400:",
            "HTTP 401:",
            "HTTP 402:",
            "HTTP 403:",
            "HTTP 404:",
        ]
        .iter()
        .any(|prefix| error.starts_with(prefix))
        {
            "failed"
        } else {
            "uncertain"
        }
        .into();
        action.finished_at = Some(now());
        action.message = redact(&error, &[api.secret()]);
        return supervisor.update_action(&approval.account_id, &approval.monitor_id, action);
    }
    action.status = "verifying".into();
    action.message = tr(
        &m.language,
        "Liara accepted the action. Checking the original symptoms now…",
        "لیارا اقدام را پذیرفت. نشانه‌های اولیه اکنون بررسی می‌شوند…",
    );
    supervisor.update_action(&approval.account_id, &approval.monitor_id, action.clone())?;
    let mut verified = false;
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Ok(mut sample) = tokio::time::timeout(
            Duration::from_secs(35),
            collect(api, &m.target, &m.settings, &m.language, Some(&action.at)),
        )
        .await
        .unwrap_or_else(|_| Err("Verification timed out".into()))
        {
            sample.signals = signals(&sample, m.samples.last(), &m.settings, &m.language);
            let old = m
                .incidents
                .iter()
                .find(|i| i.id == approval.incident_id)
                .unwrap();
            let service_health = sample
                .observations
                .iter()
                .find(|o| o.service_id == approval.service_id)
                .is_some_and(|o| {
                    !o.collection_failed
                        && (service.kind != "app"
                            || o.http_status
                                .is_some_and(|status| (200..400).contains(&status)))
                });
            let symptoms_resolved = old
                .signals
                .iter()
                .filter(|s| s.service_id == approval.service_id)
                .all(|signal| symptom_cleared(signal, &sample, &m.settings));
            let plan_confirmed = if let Some(target) = approval.target_plan_id.as_deref() {
                api.get(&base)
                    .await
                    .ok()
                    .is_some_and(|value| value[key]["planID"].as_str() == Some(target))
            } else {
                true
            };
            if service_health && symptoms_resolved && plan_confirmed {
                verified = true;
                break;
            }
        }
    }
    action.status = if verified { "verified" } else { "not_resolved" }.into();
    action.finished_at = Some(now());
    action.message = if verified {
        tr(&m.language,"The action was accepted and the follow-up checks no longer show the original symptoms. Monitoring continues.","اقدام پذیرفته شد و در بررسی پس از آن، نشانه‌های اولیه دیده نشدند. پایش ادامه دارد.")
    } else {
        tr(&m.language,"The action was accepted, but follow-up checks have not confirmed recovery. Do not assume the problem is fixed. Review the next checks and diagnosis.","اقدام پذیرفته شد، اما بررسی‌های بعدی بهبود را تایید نکردند. مشکل رفع‌شده فرض نمی‌شود؛ بررسی‌ها و تحلیل بعدی را ببینید.")
    };
    let result = supervisor.update_action(&approval.account_id, &approval.monitor_id, action)?;
    let _ = supervisor.check_now(&approval.monitor_id, &approval.account_id);
    Ok(result)
}

pub async fn reconcile_action(
    api: &Liara,
    supervisor: &Supervisor,
    account: &str,
    monitor_id: &str,
    action_id: &str,
) -> Result<Monitor> {
    let m = supervisor.get(monitor_id, account)?;
    let mut action = m
        .actions
        .iter()
        .find(|a| a.id == action_id)
        .ok_or("Action unavailable")?
        .clone();
    if !matches!(
        action.status.as_str(),
        "uncertain" | "submitting" | "verifying"
    ) {
        return Ok(m);
    }
    let service = m
        .target
        .services
        .iter()
        .find(|s| s.id == action.service_id)
        .ok_or("Service unavailable")?;
    if action.kind == "resize" {
        let (base, key) = if service.kind == "app" {
            (format!("/v1/projects/{}", service.name), "project")
        } else {
            (format!("/v1/databases/{}", service.provider_id), "database")
        };
        let detail = api.get(&base).await?;
        if detail[key]["_id"].as_str() != Some(&service.provider_id) {
            return Err("Resource identity changed; action result cannot be reconciled.".into());
        }
        if detail[key]["planID"].as_str() == action.target_plan_id.as_deref() {
            action.status = "not_resolved".into();
            action.finished_at = Some(now());
            action.message=tr(&m.language,"The upgraded plan is confirmed on Liara. Run monitoring checks to verify recovery; no mutation was repeated.","پلن ارتقایافته در لیارا تایید شد. برای بررسی بهبود، پایش را اجرا کنید؛ هیچ درخواست تغییری تکرار نشد.");
        }
    }
    if matches!(
        action.status.as_str(),
        "uncertain" | "submitting" | "verifying"
    ) {
        action.status = "uncertain".into();
        action.message=tr(&m.language,"The API does not expose an operation ID that can prove this request completed. No retry was sent. Check the Liara console; acknowledge this uncertainty before a separate approved action.","API شناسه عملیاتی برای اثبات تکمیل این درخواست ارائه نمی‌دهد. تلاشی دوباره ارسال نشد. وضعیت را در کنسول لیارا بررسی کنید؛ پیش از اقدام جداگانه، این ابهام را تایید کنید.");
    }
    supervisor.update_action(account, monitor_id, action)
}
pub fn acknowledge_action(
    supervisor: &Supervisor,
    account: &str,
    monitor_id: &str,
    action_id: &str,
) -> Result<Monitor> {
    let m = supervisor.get(monitor_id, account)?;
    let mut action = m
        .actions
        .iter()
        .find(|a| a.id == action_id)
        .ok_or("Action unavailable")?
        .clone();
    if action.status != "uncertain" {
        return Err("Only an uncertain action can be acknowledged.".into());
    }
    action.status = "acknowledged".into();
    action.finished_at = Some(now());
    action.message=tr(&m.language,"You acknowledged the uncertain result. This is not a claim of success and no cloud request was sent.","شما ابهام نتیجه را تایید کردید. این به معنی موفقیت نیست و هیچ درخواست ابری ارسال نشد.");
    supervisor.update_action(account, monitor_id, action)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn monitor() -> Monitor {
        serde_json::from_value(json!({"id":"monitor","accountId":"account","target":{"id":"target","name":"Demo","projectId":null,"runId":null,"services":[{"id":"web","name":"demo-web","kind":"app","platform":"node","providerId":"provider","url":null}]},"settings":{"intervalSeconds":15,"slowMs":2000,"healthPath":"/"},"language":"en","status":"running","startedAt":now(),"updatedAt":now(),"samples":[],"incidents":[],"actions":[],"latestDiagnosis":null,"analyzing":false,"analysisError":null})).unwrap()
    }
    fn sample(status: Option<u16>) -> Sample {
        let m = monitor();
        let mut s = failed_sample(&m);
        let o = &mut s.observations[0];
        o.collection_failed = false;
        o.provider_state = "ACTIVE".into();
        o.http_status = status;
        o.latency_ms = Some(50);
        o.logs_available = true;
        o.limitations.clear();
        s
    }
    fn approval(m: &Monitor) -> Approval {
        Approval {
            id: "approval".into(),
            monitor_id: m.id.clone(),
            incident_id: m.incidents[0].id.clone(),
            service_id: "web".into(),
            kind: "restart".into(),
            description: "Restart once".into(),
            from_monthly_price: Some(10.0),
            monthly_price: Some(10.0),
            target_plan_id: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::minutes(15)).to_rfc3339(),
            account_id: m.account_id.clone(),
            provider_id: "provider".into(),
            from_plan_id: "small".into(),
            bundle_id: "free".into(),
            release_id: Some("release".into()),
        }
    }
    #[test]
    fn summary_aggregates_replicas_and_preserves_unknowns() {
        let t = chrono::Utc::now().timestamp();
        let data = json!({"cpuUsage":[{"value":[t,"0.25"]},{"value":[t,"0.35"]}]});
        assert_eq!(metric_summary(&data, "cpuUsage"), (Some(0.6), Some(t)));
        for data in [
            json!({}),
            json!({"cpuUsage":[]}),
            json!({"cpuUsage":[{"value":[t,"NaN"]}]}),
            json!({"cpuUsage":[{"value":[t-181,"1"]}]}),
            json!({"cpuUsage":[{"value":[t+1000,"1"]}]}),
        ] {
            assert!(metric_summary(&data, "cpuUsage").0.is_none());
        }
    }
    #[test]
    fn logs_are_redacted_bounded_and_time_filtered() {
        let at = now();
        let data = json!([{"datetime":at,"message":"database failed with password=my-password bearer private-token"},{"datetime":"2020-01-01T00:00:00Z","message":"old"}]);
        let lines = bounded_logs(
            &data,
            &["my-password".into(), "private-token".into()],
            chrono::Utc::now().timestamp() - 60,
        );
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].message.contains("my-password"));
        assert!(!lines[0].message.contains("private-token"));
        let lines = bounded_logs(&json!([{"datetime":at,"message":"x".repeat(2000)}]), &[], 0);
        assert_eq!(lines[0].message.len(), 800);
    }
    #[test]
    fn cli_logs_are_normalized() {
        let t = chrono::Utc::now().timestamp();
        let data = json!({"data":[{"values":[[format!("{}",t*1_000_000_000),"error: example"]]}]});
        assert_eq!(bounded_logs(&data, &[], t - 1).len(), 1);
    }
    #[test]
    fn high_resource_signals_require_distinct_consecutive_measurements() {
        let mut a = sample(Some(200));
        let o = &mut a.observations[0];
        o.cpu_cores = Some(0.9);
        o.cpu_limit = Some(1.0);
        o.memory_bytes = Some(950.0);
        o.memory_limit_bytes = Some(1000.0);
        o.metrics_at = Some(1);
        assert!(signals(&a, None, &Settings::default(), "en").is_empty());
        assert!(signals(&a, Some(&a), &Settings::default(), "en").is_empty());
        let mut b = a.clone();
        b.observations[0].metrics_at = Some(2);
        let got = signals(&b, Some(&a), &Settings::default(), "en");
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|s| s.kind == "memory"));
    }
    #[test]
    fn latency_requires_two_checks_and_does_not_recommend_upgrade() {
        let mut s = sample(Some(200));
        s.observations[0].latency_ms = Some(4000);
        assert!(signals(&s, None, &Settings::default(), "en").is_empty());
        s.signals = signals(&s, Some(&s), &Settings::default(), "en");
        assert_eq!(s.signals[0].kind, "slow");
        assert_eq!(fallback(&s, "en").recommendations[0].kind, "investigate");
    }
    #[test]
    fn incidents_are_grouped_and_need_two_known_good_checks() {
        let mut m = monitor();
        record_sample(&mut m, sample(Some(500)));
        record_sample(&mut m, sample(Some(500)));
        assert_eq!(m.incidents.len(), 1);
        record_sample(&mut m, sample(None));
        assert!(m.incidents[0].closed_at.is_none());
        record_sample(&mut m, sample(Some(200)));
        assert!(m.incidents[0].closed_at.is_none());
        record_sample(&mut m, sample(Some(200)));
        assert!(m.incidents[0].closed_at.is_some());
        record_sample(&mut m, sample(Some(500)));
        assert_eq!(m.incidents.len(), 2);
    }
    #[test]
    fn unavailable_logs_cannot_prove_recovery() {
        let mut m = monitor();
        let mut s = sample(Some(200));
        s.observations[0].logs.push(LogLine {
            at: now(),
            message: "error: failure".into(),
        });
        record_sample(&mut m, s);
        let mut empty = sample(Some(200));
        empty.observations[0].logs_available = false;
        record_sample(&mut m, empty.clone());
        record_sample(&mut m, empty);
        assert!(m.incidents[0].closed_at.is_none());
    }
    #[test]
    fn unknown_metrics_never_resolve_a_resource_incident() {
        let signal = Signal {
            id: "web:memory".into(),
            service_id: "web".into(),
            kind: "memory".into(),
            severity: "warning".into(),
            message: "high".into(),
        };
        assert!(!symptom_cleared(
            &signal,
            &sample(Some(200)),
            &Settings::default()
        ));
    }
    #[test]
    fn approvals_bind_account_resource_incident_expiry_and_state() {
        let mut m = monitor();
        record_sample(&mut m, sample(Some(500)));
        let a = approval(&m);
        assert!(validate_approval(&a, &m, "account").is_ok());
        assert!(validate_approval(&a, &m, "other").is_err());
        let mut invalid = a.clone();
        invalid.expires_at = "2000-01-01T00:00:00Z".into();
        assert!(validate_approval(&invalid, &m, "account").is_err());
        invalid = a.clone();
        invalid.provider_id = "other".into();
        assert!(validate_approval(&invalid, &m, "account").is_err());
        m.incidents[0].closed_at = Some(now());
        assert!(validate_approval(&a, &m, "account").is_err());
    }
    #[test]
    fn approvals_are_single_use_and_stop_invalidates_them() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::new(dir.path()).unwrap());
        let supervisor = Supervisor::new(store, Arc::new(|_| {}));
        let mut m = monitor();
        record_sample(&mut m, sample(Some(500)));
        supervisor.save(&mut m).unwrap();
        let a = approval(&m);
        supervisor.put_approval(a.clone()).unwrap();
        assert!(supervisor.consume_approval(&a.id, "account").is_ok());
        assert!(supervisor.consume_approval(&a.id, "account").is_err());
        supervisor.put_approval(a.clone()).unwrap();
        supervisor.stop(&m.id, "account").unwrap();
        assert!(supervisor.consume_approval(&a.id, "account").is_err());
    }
    #[test]
    fn interrupted_sessions_are_never_reported_running_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::new(dir.path()).unwrap());
        let supervisor = Supervisor::new(store, Arc::new(|_| {}));
        let mut m = monitor();
        m.analyzing = true;
        supervisor.save(&mut m).unwrap();
        let saved = supervisor.list("account").unwrap();
        assert_eq!(saved[0].status, "interrupted");
        assert!(!saved[0].analyzing);
        assert!(!supervisor.active());
    }
    #[test]
    fn history_retention_is_bounded() {
        let mut m = monitor();
        for _ in 0..100 {
            record_sample(&mut m, sample(Some(200)));
        }
        assert_eq!(m.samples.len(), 60);
    }
    #[test]
    fn settings_reject_unsafe_paths_and_overfrequent_checks() {
        for path in ["//internal", "/ok?token=x", "/x#secret", "/x\\x", "/\r\n"] {
            let mut s = Settings::default();
            s.health_path = path.into();
            assert!(s.validate().is_err());
        }
        let mut s = Settings::default();
        s.interval_seconds = 1;
        assert!(s.validate().is_err());
        assert!(Settings::default().validate().is_ok());
    }
    #[test]
    fn health_probes_reject_private_networks() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.2",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_ip("8.8.8.8".parse().unwrap()));
    }
    #[tokio::test]
    async fn stop_cancels_collection_and_prevents_late_writes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::new(dir.path()).unwrap());
        let supervisor = Arc::new(Supervisor::new(store, Arc::new(|_| {})));
        let api = Liara::new("invalid-test-token".into(), None).unwrap();
        let m = supervisor
            .start(
                api,
                "account",
                monitor().target,
                Settings::default(),
                "en".into(),
            )
            .unwrap();
        assert!(supervisor.active());
        supervisor.stop(&m.id, "account").unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        let m = supervisor.get(&m.id, "account").unwrap();
        assert_eq!(m.status, "stopped");
        assert!(m.samples.is_empty());
        assert!(!supervisor.active());
    }
    #[test]
    fn persian_findings_are_localized() {
        let s = sample(Some(500));
        let signals = signals(&s, None, &Settings::default(), "fa");
        assert!(signals[0].message.contains("خطای"));
        assert!(fallback(&s, "fa").summary.contains("شواهد"));
    }
    #[cfg(feature = "desktop")]
    #[tokio::test]
    #[ignore = "Read-only live API check; needs existing app credentials in Secret Service and LIARA_SRE_CHECK_TARGET"]
    async fn live_read_only_check() {
        let target_name = std::env::var("LIARA_SRE_CHECK_TARGET").expect("set target name");
        let token=keyring::Entry::new("ir.liara.launch.desktop","liara-token").unwrap().get_password().expect("No saved Liara token: connect and choose secure storage in the app before this opt-in test");
        let team = keyring::Entry::new("ir.liara.launch.desktop", "liara-team-id")
            .unwrap()
            .get_password()
            .ok();
        let api = Liara::new(token, team).unwrap();
        let me = api.get("/v1/me").await.unwrap();
        let account = api.scoped_account_id(me["user"]["_id"].as_str().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let target = targets(&api, &store, &account)
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.services.iter().any(|s| s.name == target_name))
            .expect("existing target");
        let sample = collect(&api, &target, &Settings::default(), "fa", None)
            .await
            .unwrap();
        let o = sample
            .observations
            .iter()
            .find(|o| o.http_status.is_some())
            .expect("HTTP result");
        assert_eq!(o.http_status, Some(200));
        assert!(!o.collection_failed);
        assert!(o.cpu_cores.is_some());
        assert!(o.memory_bytes.is_some());
        println!(
            "READ ONLY: HTTP {:?}, metrics available, {} redacted log entries",
            o.http_status,
            o.logs.len()
        );
    }
    #[test]
    fn automatic_ai_analysis_is_triggered_by_sample_id_not_wall_clock_equality() {
        let mut m = monitor();
        let mut s = sample(Some(500));
        s.id = "first-sample".into();
        record_sample(&mut m, s);
        m.incidents[0].diagnosis.as_mut().unwrap().at = "2001-01-01T00:00:00Z".into();
        assert!(needs_analysis(&m));
        let mut s = sample(Some(500));
        s.id = "next-sample".into();
        record_sample(&mut m, s);
        assert!(!needs_analysis(&m));
    }
    #[test]
    fn upgrades_preserve_region_bundle_compute_and_disk_and_choose_lowest_verified_cost() {
        let current = PlanOption {
            id: "small".into(),
            category: "app".into(),
            label: "Small".into(),
            cpu: 0.5,
            ram: 0.5,
            disk: 5.0,
            monthly_price: Some(10.0),
            bundle_id: "basic".into(),
            region: "iran".into(),
            supports: vec!["node".into()],
        };
        let candidate =
            |id: &str, price: Option<f64>, disk: f64, cpu: f64, region: &str| PlanOption {
                id: id.into(),
                ram: 1.0,
                disk,
                cpu,
                monthly_price: price,
                region: region.into(),
                ..current.clone()
            };
        let plans = vec![
            candidate("unknown", None, 10.0, 0.5, "iran"),
            candidate("small-disk", Some(15.0), 5.0, 0.5, "iran"),
            candidate("cpu-downgrade", Some(15.0), 10.0, 0.25, "iran"),
            candidate("wrong-region", Some(15.0), 10.0, 0.5, "germany"),
            candidate("expensive", Some(40.0), 10.0, 1.0, "iran"),
            candidate("sufficient", Some(20.0), 10.0, 0.5, "iran"),
        ];
        assert_eq!(
            choose_upgrade(&plans, &current, "node", 7.0, true, false)
                .unwrap()
                .id,
            "sufficient"
        );
        assert_eq!(
            choose_upgrade(&plans, &current, "node", 7.0, true, true)
                .unwrap()
                .id,
            "expensive"
        );
        assert!(choose_upgrade(&plans, &current, "python", 7.0, true, false).is_none());
    }
}
