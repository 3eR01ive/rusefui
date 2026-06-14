//! Git-backed project repository via gitoxide. Each project is a git repo in
//! `~/.rusefui/projects/{name}/`. Saving = commit; history = git log; diff = similar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;
use gix::objs::{tree, BlobRef, Tree};
use serde::{Deserialize, Serialize};

use crate::project::RusefuiProject;
use crate::project_timeline::ProjectTimeline;

pub const PROJECT_JSON: &str = "project.json";
pub const SCRIPTS_DIR: &str = "scripts";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitSummary {
    pub id: String,
    pub short_id: String,
    pub message: String,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListEntry {
    pub dir: String,
    pub name: String,
}

pub struct ProjectGitRepo {
    pub dir: PathBuf,
}

impl ProjectGitRepo {
    pub fn projects_root() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rusefui")
            .join("projects")
    }

    pub fn sanitize_name(name: &str) -> String {
        let s: String = name
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
                c => c,
            })
            .collect();
        let s = s.trim().to_string();
        if s.is_empty() { "project".to_string() } else { s }
    }

    pub fn unique_dir_for_name(name: &str) -> PathBuf {
        let root = Self::projects_root();
        let base = Self::sanitize_name(name);
        let candidate = root.join(&base);
        if !candidate.exists() {
            return candidate;
        }
        for i in 2u32..=999 {
            let c = root.join(format!("{base}-{i}"));
            if !c.exists() {
                return c;
            }
        }
        root.join(format!("{base}-{}", now_ms()))
    }

    pub fn create(name: &str) -> Result<(Self, RusefuiProject), String> {
        let dir = Self::unique_dir_for_name(name);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        gix::init(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(".gitignore"), "ui_panels/\n").map_err(|e| e.to_string())?;
        let doc = RusefuiProject::new_named(name);
        let repo = Self { dir };
        repo.write_doc_and_commit(&doc, "Начало проекта")?;
        Ok((repo, doc))
    }

    pub fn open(dir: &Path) -> Result<(Self, RusefuiProject), String> {
        if !dir.join(".git").is_dir() {
            return Err(format!("Не git-репозиторий: {}", dir.display()));
        }
        let json_path = dir.join(PROJECT_JSON);
        let text = std::fs::read_to_string(&json_path)
            .map_err(|e| format!("project.json не найден: {e}"))?;
        let doc: RusefuiProject =
            serde_json::from_str(&text).map_err(|e| format!("Некорректный project.json: {e}"))?;
        Ok((Self { dir: dir.to_path_buf() }, doc))
    }

    pub fn import_legacy(json: &str, name: &str) -> Result<(Self, RusefuiProject), String> {
        let doc: RusefuiProject = serde_json::from_str(json)
            .map_err(|e| format!("Некорректный JSON проекта: {e}"))?;
        let dir = Self::unique_dir_for_name(name);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        gix::init(&dir).map_err(|e| e.to_string())?;
        let repo = Self { dir };
        repo.write_doc_and_commit(&doc, "Импорт из legacy формата")?;
        Ok((repo, doc))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn name(&self) -> String {
        self.dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string()
    }

    /// Serialize doc → project.json on disk + git commit (includes scripts/).
    pub fn write_doc_and_commit(&self, doc: &RusefuiProject, message: &str) -> Result<String, String> {
        let json =
            serde_json::to_string_pretty(doc).map_err(|e| format!("Сериализация: {e}"))?;
        std::fs::write(self.dir.join(PROJECT_JSON), &json).map_err(|e| e.to_string())?;
        let scripts = self.scan_scripts().unwrap_or_default();
        self.git_commit_all(&json, &scripts, message)
    }

    /// Read all .lua files from scripts/ dir. Returns (filename, content) sorted by name.
    fn scan_scripts(&self) -> Result<Vec<(String, String)>, String> {
        let scripts_dir = self.dir.join(SCRIPTS_DIR);
        if !scripts_dir.is_dir() {
            return Ok(vec![]);
        }
        let mut result = Vec::new();
        for entry in std::fs::read_dir(&scripts_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("lua") {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let content =
                    std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                result.push((filename, content));
            }
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }

    fn git_commit_all(
        &self,
        project_json: &str,
        scripts: &[(String, String)],
        message: &str,
    ) -> Result<String, String> {
        let repo = gix::open(&self.dir).map_err(|e| e.to_string())?;

        let json_blob_id = repo
            .write_object(BlobRef { data: project_json.as_bytes() })
            .map_err(|e| e.to_string())?
            .detach();

        let mut root_entries = vec![tree::Entry {
            mode: tree::EntryKind::Blob.into(),
            filename: PROJECT_JSON.into(),
            oid: json_blob_id,
        }];

        if !scripts.is_empty() {
            let mut script_entries: Vec<tree::Entry> = scripts
                .iter()
                .map(|(filename, content)| {
                    let blob_id = repo
                        .write_object(BlobRef { data: content.as_bytes() })
                        .map(|id| id.detach())
                        .unwrap_or(gix::ObjectId::null(gix::hash::Kind::Sha1));
                    tree::Entry {
                        mode: tree::EntryKind::Blob.into(),
                        filename: filename.as_str().into(),
                        oid: blob_id,
                    }
                })
                .collect();
            script_entries.sort_by(|a, b| a.filename.cmp(&b.filename));
            let scripts_tree_id = repo
                .write_object(&Tree { entries: script_entries })
                .map_err(|e| e.to_string())?
                .detach();
            root_entries.push(tree::Entry {
                mode: tree::EntryKind::Tree.into(),
                filename: SCRIPTS_DIR.into(),
                oid: scripts_tree_id,
            });
        }

        root_entries.sort_by(|a, b| a.filename.cmp(&b.filename));
        let root_tree_id = repo
            .write_object(&Tree { entries: root_entries })
            .map_err(|e| e.to_string())?
            .detach();

        let sig = gix::actor::Signature {
            name: "rusefui".into(),
            email: "rusefui@local".into(),
            time: gix::date::Time::now_local_or_utc(),
        };
        let mut tbuf_a = gix::date::parse::TimeBuf::default();
        let mut tbuf_c = gix::date::parse::TimeBuf::default();
        let parent = repo.head_id().ok().map(|id| id.detach());
        let commit_id = repo
            .commit_as(
                sig.to_ref(&mut tbuf_c),
                sig.to_ref(&mut tbuf_a),
                "HEAD",
                message,
                root_tree_id,
                parent,
            )
            .map_err(|e| e.to_string())?;

        Ok(commit_id.to_string())
    }

    pub fn history(&self) -> Result<Vec<CommitSummary>, String> {
        let repo = gix::open(&self.dir).map_err(|e| e.to_string())?;
        let head_id = match repo.head_id() {
            Ok(id) => id.detach(),
            Err(_) => return Ok(vec![]),
        };

        let walk = repo.rev_walk([head_id]).all().map_err(|e| e.to_string())?;
        let mut commits = Vec::new();

        for info in walk {
            let info = info.map_err(|e| e.to_string())?;
            let commit = repo.find_commit(info.id).map_err(|e| e.to_string())?;
            let decoded = commit.decode().map_err(|e| e.to_string())?;

            let msg = String::from_utf8_lossy(decoded.message.as_ref()).trim().to_string();
            let time_sec = decoded.committer().map(|c| c.seconds()).unwrap_or(0);
            let id = info.id.to_string();
            let short_id: String = id.chars().take(8).collect();

            commits.push(CommitSummary {
                id,
                short_id,
                message: msg,
                timestamp_ms: time_sec * 1000,
            });
        }

        Ok(commits)
    }

    /// Read all text files from a commit tree. Returns map: "project.json" | "scripts/foo.lua" → content.
    pub fn all_blobs_at_commit(&self, commit_id_str: &str) -> Result<HashMap<String, String>, String> {
        let repo = gix::open(&self.dir).map_err(|e| e.to_string())?;
        let commit_id = gix::ObjectId::from_hex(commit_id_str.as_bytes())
            .map_err(|_| format!("Неверный id: {commit_id_str}"))?;
        let commit = repo.find_commit(commit_id).map_err(|e| e.to_string())?;
        let decoded = commit.decode().map_err(|e| e.to_string())?;
        let root_tree = repo
            .find_object(decoded.tree())
            .map_err(|e| e.to_string())?
            .into_tree();
        let root_ref = root_tree.decode().map_err(|e| e.to_string())?;

        let mut files = HashMap::new();
        for entry in &root_ref.entries {
            let name = String::from_utf8_lossy(entry.filename.as_bytes()).into_owned();
            if name == PROJECT_JSON {
                let blob = repo
                    .find_object(entry.oid.to_owned())
                    .map_err(|e| e.to_string())?
                    .into_blob();
                let content = String::from_utf8(blob.data.clone()).map_err(|e| e.to_string())?;
                files.insert(name, content);
            } else if name == SCRIPTS_DIR {
                let scripts_tree = repo
                    .find_object(entry.oid.to_owned())
                    .map_err(|e| e.to_string())?
                    .into_tree();
                let scripts_ref = scripts_tree.decode().map_err(|e| e.to_string())?;
                for se in &scripts_ref.entries {
                    let sname = String::from_utf8_lossy(se.filename.as_bytes()).into_owned();
                    let blob = repo
                        .find_object(se.oid.to_owned())
                        .map_err(|e| e.to_string())?
                        .into_blob();
                    let content =
                        String::from_utf8(blob.data.clone()).map_err(|e| e.to_string())?;
                    files.insert(format!("{SCRIPTS_DIR}/{sname}"), content);
                }
            }
        }
        Ok(files)
    }

    /// Unified diff between two commits (all files).
    pub fn diff_commits(&self, from_id: &str, to_id: &str) -> Result<String, String> {
        let from = self.all_blobs_at_commit(from_id)?;
        let to = self.all_blobs_at_commit(to_id)?;
        Ok(diff_file_maps(&from, &to))
    }

    /// Unified diff between a commit and current working state (project.json + scripts/).
    pub fn diff_working(&self, from_id: &str, doc: &RusefuiProject) -> Result<String, String> {
        let from = self.all_blobs_at_commit(from_id)?;
        let mut to = HashMap::new();
        to.insert(
            PROJECT_JSON.to_string(),
            serde_json::to_string_pretty(doc).unwrap_or_default(),
        );
        for (filename, content) in self.scan_scripts().unwrap_or_default() {
            to.insert(format!("{SCRIPTS_DIR}/{filename}"), content);
        }
        Ok(diff_file_maps(&from, &to))
    }

    /// Restore project state from a commit: writes project.json + script files.
    pub fn checkout(&self, commit_id_str: &str) -> Result<RusefuiProject, String> {
        let files = self.all_blobs_at_commit(commit_id_str)?;

        let json = files
            .get(PROJECT_JSON)
            .ok_or("project.json не найден в коммите")?;
        std::fs::write(self.dir.join(PROJECT_JSON), json).map_err(|e| e.to_string())?;

        // Recreate scripts/ from commit snapshot
        let scripts_dir = self.dir.join(SCRIPTS_DIR);
        if scripts_dir.is_dir() {
            std::fs::remove_dir_all(&scripts_dir).map_err(|e| e.to_string())?;
        }
        for (key, content) in &files {
            if let Some(script_name) = key.strip_prefix(&format!("{SCRIPTS_DIR}/")) {
                std::fs::create_dir_all(&scripts_dir).map_err(|e| e.to_string())?;
                std::fs::write(scripts_dir.join(script_name), content)
                    .map_err(|e| e.to_string())?;
            }
        }

        serde_json::from_str(json).map_err(|e| format!("Некорректный project.json: {e}"))
    }

    pub fn fork_without_timeline(
        doc: &RusefuiProject,
        new_name: &str,
    ) -> Result<(Self, RusefuiProject), String> {
        let mut new_doc = doc.clone();
        let base = new_doc.name.trim().to_string();
        new_doc.name = if !new_name.is_empty() {
            new_name.to_string()
        } else if base.ends_with("(копия)") {
            base
        } else {
            format!("{base} (копия)")
        };
        new_doc.timeline = ProjectTimeline::default();
        new_doc.scripts = Vec::new();
        let t = now_ms();
        new_doc.created_at_ms = t;
        new_doc.updated_at_ms = t;

        let dir = Self::unique_dir_for_name(&new_doc.name);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        gix::init(&dir).map_err(|e| e.to_string())?;
        let repo = Self { dir };
        repo.write_doc_and_commit(&new_doc, "Форк проекта")?;
        Ok((repo, new_doc))
    }

    pub fn list_all() -> Vec<ProjectListEntry> {
        let root = Self::projects_root();
        let Ok(rd) = std::fs::read_dir(&root) else { return vec![] };
        let mut entries: Vec<ProjectListEntry> = rd
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                if !path.is_dir() || !path.join(".git").is_dir() {
                    return None;
                }
                let name = path.file_name()?.to_str()?.to_string();
                Some(ProjectListEntry { dir: path.display().to_string(), name })
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    // -----------------------------------------------------------------------
    // Script file helpers
    // -----------------------------------------------------------------------

    pub fn scripts_dir(&self) -> PathBuf {
        self.dir.join(SCRIPTS_DIR)
    }

    pub fn script_path(&self, id: &str) -> PathBuf {
        self.scripts_dir().join(format!("{id}.lua"))
    }

    pub fn write_script_content(&self, id: &str, content: &str) -> Result<(), String> {
        let path = self.script_path(id);
        std::fs::create_dir_all(self.scripts_dir()).map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| e.to_string())
    }

    pub fn read_script_content(&self, id: &str) -> Result<String, String> {
        std::fs::read_to_string(self.script_path(id))
            .map_err(|e| format!("Не удалось прочитать скрипт {id}: {e}"))
    }

    /// Commits where `scripts/{id}.lua` changed (walk all history, compare blob OIDs).
    pub fn script_history(&self, script_id: &str) -> Result<Vec<CommitSummary>, String> {
        let repo = gix::open(&self.dir).map_err(|e| e.to_string())?;
        let head_id = match repo.head_id() {
            Ok(id) => id.detach(),
            Err(_) => return Ok(vec![]),
        };

        let walk = repo.rev_walk([head_id]).all().map_err(|e| e.to_string())?;
        let mut commits = Vec::new();
        let mut prev_oid: Option<gix::ObjectId> = None;
        let mut first = true;

        for info in walk {
            let info = info.map_err(|e| e.to_string())?;
            let commit = repo.find_commit(info.id).map_err(|e| e.to_string())?;
            let (tree_id, msg, time_sec) = {
                let decoded = commit.decode().map_err(|e| e.to_string())?;
                (
                    decoded.tree(),
                    String::from_utf8_lossy(decoded.message.as_ref()).trim().to_string(),
                    decoded.committer().map(|c| c.seconds()).unwrap_or(0),
                )
            };

            let current_oid = script_blob_oid_in_tree(&repo, tree_id, script_id)?;
            let changed = if first { current_oid.is_some() } else { current_oid != prev_oid };

            if changed {
                let id_str = info.id.to_string();
                let short_id: String = id_str.chars().take(8).collect();
                commits.push(CommitSummary {
                    id: id_str,
                    short_id,
                    message: msg,
                    timestamp_ms: time_sec * 1000,
                });
            }
            prev_oid = current_oid;
            first = false;
        }
        Ok(commits)
    }

    /// Unified diff of a single script file between two commits (or commit → working copy).
    pub fn script_diff(
        &self,
        script_id: &str,
        from_id: &str,
        to_id: Option<&str>,
    ) -> Result<String, String> {
        let key = format!("{SCRIPTS_DIR}/{script_id}.lua");
        let from_files = self.all_blobs_at_commit(from_id)?;
        let old = from_files.get(&key).map(String::as_str).unwrap_or("");

        let new_owned = match to_id {
            Some(id) => {
                let to_files = self.all_blobs_at_commit(id)?;
                to_files.get(&key).cloned().unwrap_or_default()
            }
            None => self.read_script_content(script_id).unwrap_or_default(),
        };

        let d = similar::TextDiff::from_lines(old, &new_owned);
        Ok(d.unified_diff()
            .header(&format!("a/{key}"), &format!("b/{key}"))
            .to_string())
    }

    /// Read script content from a specific commit (returns error if not found in that commit).
    pub fn script_content_at_commit(
        &self,
        script_id: &str,
        commit_id_str: &str,
    ) -> Result<String, String> {
        let key = format!("{SCRIPTS_DIR}/{script_id}.lua");
        let files = self.all_blobs_at_commit(commit_id_str)?;
        files
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("Скрипт {script_id} не найден в {commit_id_str}"))
    }

    pub fn delete_script_file(&self, id: &str) -> Result<(), String> {
        let path = self.script_path(id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Find the blob OID of `scripts/{script_id}.lua` in a tree (returns None if absent).
fn script_blob_oid_in_tree(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    script_id: &str,
) -> Result<Option<gix::ObjectId>, String> {
    let filename = format!("{script_id}.lua");
    let root_tree =
        repo.find_object(tree_id).map_err(|e| e.to_string())?.into_tree();

    let scripts_tree_oid = {
        let root_ref = root_tree.decode().map_err(|e| e.to_string())?;
        root_ref
            .entries
            .iter()
            .find(|e| e.filename.as_bytes() == SCRIPTS_DIR.as_bytes())
            .map(|e| e.oid.to_owned())
    };
    let scripts_oid = match scripts_tree_oid {
        Some(o) => o,
        None => return Ok(None),
    };

    let scripts_tree = repo.find_object(scripts_oid).map_err(|e| e.to_string())?.into_tree();
    let result = {
        let scripts_ref = scripts_tree.decode().map_err(|e| e.to_string())?;
        scripts_ref
            .entries
            .iter()
            .find(|e| e.filename.as_bytes() == filename.as_bytes())
            .map(|e| e.oid.to_owned())
    };
    Ok(result)
}

fn diff_file_maps(from: &HashMap<String, String>, to: &HashMap<String, String>) -> String {
    let mut keys: Vec<String> = from.keys().chain(to.keys()).cloned().collect();
    keys.sort();
    keys.dedup();

    let mut result = String::new();
    for key in &keys {
        let old = from.get(key).map(String::as_str).unwrap_or("");
        let new = to.get(key).map(String::as_str).unwrap_or("");
        if old != new {
            let d = similar::TextDiff::from_lines(old, new);
            result.push_str(
                &d.unified_diff()
                    .header(&format!("a/{key}"), &format!("b/{key}"))
                    .to_string(),
            );
        }
    }
    result
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
