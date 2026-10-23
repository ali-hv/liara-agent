use crate::models::Result;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

pub fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
pub fn safe_name(name: &str) -> bool {
    Regex::new(r"^[a-z][a-z0-9-]{2,49}$").unwrap().is_match(name) && !name.ends_with('-')
}
pub fn slug(name: &str) -> String {
    let raw = Regex::new("[^a-z0-9-]+").unwrap().replace_all(&name.to_lowercase(), "-").trim_matches('-').to_string();
    let mut s = if raw.chars().next().is_some_and(|x| x.is_ascii_alphabetic()) {raw} else {format!("app-{raw}")};
    s.truncate(35); if s.len()<3 {s.push_str("-app");} s.trim_end_matches('-').into()
}
pub fn scoped(root: &Path, relative: &str) -> Result<PathBuf> {
    let p = Path::new(relative);
    if p.is_absolute() || p.components().any(|c| matches!(c,Component::ParentDir|Component::RootDir|Component::Prefix(_))) { return Err("Path must stay inside the selected project.".into()); }
    let root = root.canonicalize().map_err(|e|e.to_string())?;
    let target = root.join(p);
    if target.exists() {
        let actual = target.canonicalize().map_err(|e|e.to_string())?;
        if !actual.starts_with(&root) {return Err("Symlink points outside the selected project.".into());}
        Ok(actual)
    } else {
        let mut ancestor=target.as_path();
        while !ancestor.exists() {ancestor=ancestor.parent().ok_or("Invalid path")?;}
        if !ancestor.canonicalize().map_err(|e|e.to_string())?.starts_with(&root) {return Err("Path escapes the selected project.".into());}
        Ok(target)
    }
}
pub fn sensitive_path(path: &Path) -> bool {
    path.components().any(|c| {
        let s=c.as_os_str().to_string_lossy().to_lowercase();
        matches!(s.as_str(),".git"|".ssh"|".aws"|".gnupg"|"node_modules"|"vendor"|".venv"|"venv"|"__pycache__"|"target"|".next"|".cache"|".idea"|".vscode"|".npmrc"|".pypirc"|"credentials.json"|"id_rsa"|"id_ed25519")
        || (s.starts_with(".env") && !matches!(s.as_str(),".env.example"|".env.sample"|".env.template"))
        || s.ends_with(".pem") || s.ends_with(".key") || s.ends_with(".p12") || s.ends_with(".sqlite") || s.ends_with(".sqlite3") || s.ends_with(".log")
    })
}
pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out=text.to_string();
    let mut values=secrets.iter().filter(|s|!s.is_empty()).collect::<Vec<_>>();
    values.sort_by_key(|s|std::cmp::Reverse(s.len()));
    for s in values {out=out.replace(s,"[REDACTED]");}
    for pattern in [r"(?i)Bearer\s+[A-Za-z0-9._~+/=-]+", r"\bsk-[A-Za-z0-9_-]{12,}",r#"(?i)(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp)://[^\s"']+"#,r"(?im)(?:api[_-]?key|secret|password|token|access[_-]?key)\s*[:=]\s*[^\n,;]+"] {
        // All patterns are constants; an invalid pattern must never bypass the other filters.
        if let Ok(re)=Regex::new(pattern) {out=re.replace_all(&out,"[REDACTED]").into_owned();}
    }
    out
}
pub fn sanitize_json(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map)=> Value::Object(map.iter().map(|(k,v)| (k.clone(), if ["password","secret","token","accesskey","contactinfo","nationalcode","phone"].iter().any(|s|k.to_lowercase().contains(s)) {Value::String("[REDACTED]".into())}else{sanitize_json(v)})).collect()),
        Value::Array(a)=>Value::Array(a.iter().map(sanitize_json).collect()),
        Value::String(s)=>Value::String(redact(s,&[])), _=>value.clone()
    }
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn boundaries(){let d=tempfile::tempdir().unwrap();assert!(scoped(d.path(),"../secret").is_err());assert!(scoped(d.path(),"/etc/passwd").is_err());std::os::unix::fs::symlink("/etc",d.path().join("escape")).unwrap();assert!(scoped(d.path(),"escape/passwd").is_err());}
 #[test] fn private_files(){for p in [".env","sub/.env.production",".ssh/id_rsa","node_modules/x","a.pem"]{assert!(sensitive_path(Path::new(p)));}assert!(!sensitive_path(Path::new(".env.example")));}
 #[test] fn secrets_are_redacted(){let s=redact("Bearer abc.def.ghi\npassword=hello\nsk-1234567890123456",&[]);assert!(!s.contains("abc.def"));assert!(!s.contains("hello"));assert!(!s.contains("123456"));}
}
