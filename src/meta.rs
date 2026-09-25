use crate::{Located, MetaUpdate, Origin};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Default)]
pub struct SessionMeta {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub agent_version: Option<String>,
    pub origin: Origin,
    pub started_at: Option<DateTime<Utc>>,
}
impl SessionMeta {
    /// Applies explicit updates; origin uses Subagent > Exec > Ide > Interactive.
    /// Raw conflicting provenance remains available in the input event stream.
    pub fn apply(&mut self, update: &Located<MetaUpdate>) {
        let u = &update.value;
        if u.session_id.is_some() && self.session_id.is_some() && u.session_id != self.session_id {
            *self = Self::default();
        }
        if u.session_id.is_some() {
            self.session_id.clone_from(&u.session_id);
        }
        if u.cwd.is_some() {
            self.cwd.clone_from(&u.cwd);
        }
        if u.git_branch.is_some() {
            self.git_branch.clone_from(&u.git_branch);
        }
        if u.model.is_some() {
            self.model.clone_from(&u.model);
        }
        if u.agent_version.is_some() {
            self.agent_version.clone_from(&u.agent_version);
        }
        if let Some(origin) = u.origin
            && origin.priority() > self.origin.priority()
        {
            self.origin = origin;
        }
        if self.started_at.is_none() {
            self.started_at = update.at;
        }
    }
}
