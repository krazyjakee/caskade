//! What a change needs from the signed-in account (#205). Admins may do
//! everything; a standard account what its [`super::Permissions`] name. Reading
//! is never checked, and neither is the app's own background work (scans,
//! metadata, updates, the emulator watch): only what an account asks for.

use super::{Account, AccountRole, AccountService};
use crate::{Error, Result, read};

/// A permission group: a Settings section, installing, or admin-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Changing a Settings section, one of [`super::SETTINGS_SECTIONS`].
    Settings(&'static str),
    /// Bringing games in (install requests, ROM scans and imports, adding
    /// games by hand), uninstalling and moving them. Updates of installed
    /// games need none.
    Install,
    /// What only an admin changes.
    Admin,
}

impl Permission {
    /// What changing the settings `namespace` needs: the Settings section
    /// that shows it. Each plugin's settings fall under `plugins`; a
    /// namespace no section owns is for admins.
    pub fn for_namespace(namespace: &str) -> Self {
        let section = match namespace {
            "core.general" | "core.console" | "core.onboarding" | "core.metadata"
            | "core.appUpdates" => "general",
            "core.discovery" | "discovery.import" => "library-discovery",
            "core.launchers" => "launchers",
            "core.emulation" | "core.romm" => "emulators",
            "core.appearance" => "appearance",
            "core.storage" | "storage.roots" => "storage",
            "core.installer" | "core.updates" => "installer",
            _ if namespace.starts_with("plugins.") => "plugins",
            _ => return Self::Admin,
        };
        Self::Settings(section)
    }
}

impl AccountService {
    /// The signed-in account. Every change is made as one, so with nobody
    /// signed in only signing in is allowed.
    pub fn signed_in(&self) -> Result<Account> {
        read(&self.active)
            .clone()
            .ok_or_else(|| Error::Forbidden(crate::tr!("sign in to an account first")))
    }

    /// Whether the signed-in account may do what needs `permission`;
    /// [`Error::Forbidden`] says why not.
    pub fn require(&self, permission: Permission) -> Result<()> {
        let account = self.signed_in()?;
        let allowed = account.role == AccountRole::Admin
            || match permission {
                Permission::Settings(section) => {
                    account.permissions.settings.iter().any(|s| s == section)
                }
                Permission::Install => account.permissions.install,
                Permission::Admin => false,
            };
        if allowed {
            Ok(())
        } else {
            Err(forbidden(permission))
        }
    }
}

pub(super) fn forbidden(permission: Permission) -> Error {
    Error::Forbidden(match permission {
        Permission::Settings(section) => crate::tr!(
            "your account can't change the {section} settings; ask an admin",
            section = label(section)
        ),
        Permission::Install => {
            crate::tr!("your account can't install, uninstall or move games; ask an admin")
        }
        Permission::Admin => crate::tr!("only an admin can change that"),
    })
}

/// A section's name in Settings.
fn label(section: &str) -> String {
    match section {
        "general" => crate::trp!("settings section", "General"),
        "library-discovery" => crate::trp!("settings section", "Library Discovery"),
        "launchers" => crate::trp!("settings section", "Launchers"),
        "emulators" => crate::trp!("settings section", "Emulation"),
        "appearance" => crate::trp!("settings section", "Appearance"),
        "storage" => crate::trp!("settings section", "Storage"),
        "installer" => crate::trp!("settings section", "Installer"),
        "plugins" => crate::trp!("settings section", "Plugins"),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_namespace_a_section_shows_maps_to_that_section() {
        for (namespace, section) in [
            ("core.general", "general"),
            ("core.console", "general"),
            ("core.discovery", "library-discovery"),
            ("core.launchers", "launchers"),
            ("core.romm", "emulators"),
            ("core.appearance", "appearance"),
            ("storage.roots", "storage"),
            ("core.updates", "installer"),
            ("plugins.example.pack", "plugins"),
        ] {
            assert_eq!(
                Permission::for_namespace(namespace),
                Permission::Settings(section),
                "{namespace}"
            );
            assert!(crate::accounts::SETTINGS_SECTIONS.contains(&section));
        }
        assert_eq!(Permission::for_namespace("core.unknown"), Permission::Admin);
        // The rating board is for every account: admins choose it.
        assert_eq!(
            Permission::for_namespace("core.accounts"),
            Permission::Admin
        );
    }

    #[tokio::test]
    async fn admins_may_do_everything_and_standard_accounts_what_they_were_given() {
        use crate::{
            accounts::{AccountAccess, AccountUpdate, DEFAULT_AVATAR, NewAccount, Permissions},
            bus::Bus,
            db::LibraryStore,
        };
        let store = LibraryStore::in_memory().await.unwrap();
        let accounts = AccountService::open(store, Bus::default()).await.unwrap();
        let admin = accounts.signed_in().unwrap();
        for permission in [
            Permission::Settings("general"),
            Permission::Install,
            Permission::Admin,
        ] {
            accounts.require(permission).unwrap();
        }
        accounts
            .set_pin(&admin.id, Some("2468".into()))
            .await
            .unwrap();
        let access = |settings: Vec<String>, install| AccountAccess {
            role: AccountRole::Standard,
            permissions: Permissions { settings, install },
            max_age: None,
            unrated_needs_pin: false,
        };
        let kid = accounts
            .create(NewAccount {
                name: "Kid".into(),
                avatar: DEFAULT_AVATAR.into(),
                access: access(Vec::new(), false),
                pin: None,
            })
            .await
            .unwrap();
        accounts.sign_in(&kid.id, None).await.unwrap();
        for permission in [
            Permission::Settings("appearance"),
            Permission::Install,
            Permission::Admin,
        ] {
            let error = accounts.require(permission).unwrap_err();
            assert!(matches!(error, Error::Forbidden(_)), "{error}");
        }
        // Allowed once an admin grants them.
        accounts
            .sign_in(&admin.id, Some("2468".into()))
            .await
            .unwrap();
        let grant = AccountUpdate {
            name: kid.name.clone(),
            avatar: kid.avatar.clone(),
            access: Some(access(vec!["appearance".into()], true)),
        };
        accounts.update(&kid.id, grant).await.unwrap();
        accounts.sign_in(&kid.id, None).await.unwrap();
        accounts
            .require(Permission::Settings("appearance"))
            .unwrap();
        accounts.require(Permission::Install).unwrap();
        assert!(accounts.require(Permission::Settings("general")).is_err());
        assert!(accounts.require(Permission::Admin).is_err());

        accounts.sign_out();
        let error = accounts.require(Permission::Install).unwrap_err();
        assert_eq!(error.to_string(), "sign in to an account first");
    }

    #[test]
    fn what_an_account_may_not_do_reads_in_the_language_shown() {
        const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "your account can't change the {section} settings; ask an admin"
msgstr "ditt konto får inte ändra inställningarna för {section}; fråga en administratör"

msgctxt "settings section"
msgid "Appearance"
msgstr "Utseende"
"#;
        let error = forbidden(Permission::Settings("appearance"));
        assert_eq!(
            crate::error::ErrorReport::from(&error).summary,
            "Your account can't change the Appearance settings; ask an admin"
        );
        let _sv = crate::i18n::scope_catalog("sv", SV);
        let error = forbidden(Permission::Settings("appearance"));
        let report = crate::error::ErrorReport::from(&error);
        assert_eq!(report.code, crate::error::ErrorCode::Forbidden);
        assert_eq!(
            report.summary,
            "Ditt konto får inte ändra inställningarna för Utseende; fråga en administratör"
        );
    }
}
