//! The forge integration an instance runs, for display (`specs/configuration-management`).

/// Which forge integration serves the projects of this instance. It follows the plugin name from
/// the instance configuration, not the host of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeKind {
    Forgejo,
    GitHub,
    /// A plugin the UI has no icon for; shown with the generic Git icon and the plugin name.
    Other(String),
}

impl ForgeKind {
    /// The kind of the forge plugin with this name (`plugins.<name>` in the instance config).
    pub fn from_plugin(name: &str) -> Self {
        match name {
            "forgejo" => Self::Forgejo,
            "github" => Self::GitHub,
            other => Self::Other(other.to_string()),
        }
    }

    /// Name shown next to the icon.
    pub fn label(&self) -> &str {
        match self {
            Self::Forgejo => "Forgejo",
            Self::GitHub => "GitHub",
            Self::Other(name) => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_plugins_have_their_own_kind_and_others_keep_their_name() {
        assert_eq!(ForgeKind::from_plugin("forgejo"), ForgeKind::Forgejo);
        assert_eq!(ForgeKind::from_plugin("github").label(), "GitHub");
        let other = ForgeKind::from_plugin("gitea");
        assert_eq!(other, ForgeKind::Other("gitea".into()));
        assert_eq!(other.label(), "gitea");
        assert_eq!(ForgeKind::Forgejo.label(), "Forgejo");
    }
}
