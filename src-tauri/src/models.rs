use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Result<T> = std::result::Result<T, String>;
pub fn now() -> String { chrono::Utc::now().to_rfc3339() }
pub fn id() -> String { uuid::Uuid::new_v4().to_string() }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanOption {
    pub id: String, pub category: String, pub label: String, pub ram: f64, pub cpu: f64,
    pub disk: f64, pub monthly_price: Option<f64>, pub bundle_id: String,
    pub region: String, pub supports: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String, pub name: String, pub email: String, pub balance: Option<f64>,
    pub credential_storage: String, pub plans: Vec<PlanOption>, pub networks: Vec<Value>,
    pub projects: Vec<Value>, pub databases: Vec<Value>, pub buckets: Vec<Value>,
    pub database_versions: Value, pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedService {
    pub id: String, pub name: String, pub kind: String, pub platform: String,
    pub root: String, pub port: u16, pub image: Option<String>, pub evidence: Vec<String>,
    pub env_keys: Vec<String>, pub dependencies: Vec<String>, pub warnings: Vec<String>,
    #[serde(default)] pub volumes: Vec<VolumeMount>,
    pub build_command: Option<String>, pub start_command: Option<String>,
    #[serde(default)] pub command: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all="camelCase")]
pub struct VolumeMount {
    pub id: String, pub source: String, pub target: String, pub kind: String,
    pub read_only: bool, #[serde(default)] pub resolution: Option<String>,
    #[serde(default)] pub disk_name: Option<String>, #[serde(default)] pub size_gb: Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAnalysis {
    pub id: String, pub path: String, pub name: String, pub services: Vec<DetectedService>,
    pub git_branch: Option<String>, pub git_dirty: bool, pub file_count: usize,
    pub warnings: Vec<String>, pub analyzed_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedFile { pub path: String, pub content: String, pub reason: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppResize {pub from_plan_id:String,pub from_monthly_price:f64,pub target_monthly_price:f64,pub required_gb:f64,pub target_capacity_gb:f64}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePlan {
    #[serde(default)] pub resize: Option<AppResize>,
    pub id: String, pub name: String, pub kind: String, pub platform: String, pub root: String,
    pub port: u16, pub image: Option<String>, pub plan_id: String, pub bundle_id: String,
    pub monthly_price: Option<f64>, pub existing_id: Option<String>, pub version: Option<String>,
    #[serde(default)] pub local_env_keys: Vec<String>, pub env_keys: Vec<String>, pub bindings: BTreeMap<String, String>, pub dependencies: Vec<String>,
    #[serde(default)] pub volumes: Vec<VolumeMount>,
    pub files: Vec<GeneratedFile>, pub source_hash: String, pub rationale: String,
    #[serde(default)] pub command: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentPlan {
    pub id: String, pub project_id: String, pub account_id: String, pub revision: String,
    pub network: Option<String>, pub services: Vec<ServicePlan>, pub summary: String,
    pub warnings: Vec<String>, pub blockers: Vec<String>, pub created_at: String,
    pub language: String, pub allow_config_repairs: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String, pub plan_id: String, pub revision: String, pub actions: Vec<String>,
    pub monthly_price: Option<f64>, pub expires_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProgress {
    pub service_id: String, pub resource_id: Option<String>, pub source_id: Option<String>,
    pub release_id: Option<String>, pub phase: String, pub url: Option<String>,
    pub repair_attempts: u8, pub request_pending: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvent {
    pub at: String, pub level: String, pub service_id: Option<String>, pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRun {
    pub id: String, pub plan: DeploymentPlan, pub status: String, pub started_at: String,
    pub updated_at: String, pub network_id: Option<String>, pub network_pending: bool, pub resources: Vec<ResourceProgress>, pub events: Vec<RunEvent>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceChoice {
    #[serde(default)] pub plan_locked: bool,
    pub name: Option<String>, pub plan_id: Option<String>, pub bundle_id: Option<String>,
    pub existing_id: Option<String>, pub port: Option<u16>, pub version: Option<String>,
    #[serde(default)] pub bindings: BTreeMap<String,String>,
    #[serde(default)] pub volumes: BTreeMap<String,VolumeChoice>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all="camelCase")]
pub struct VolumeChoice { pub resolution: Option<String>, pub disk_name: Option<String>, pub size_gb: Option<u32> }
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlanningQuestion {pub id:String,pub question:String,pub hint:String,#[serde(default)]pub options:Vec<String>}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlanChoices {
    #[serde(default)] pub services: BTreeMap<String,ServiceChoice>, pub network: Option<String>,
    #[serde(default)] pub allow_config_repairs: bool,
    #[serde(default)] pub answers:BTreeMap<String,String>,
    #[serde(default)] pub guided:bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage { pub role: String, pub content: String }
pub type Secrets = BTreeMap<String, BTreeMap<String,String>>;
