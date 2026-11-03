//! A yes/no question in the status bar, answered with `y` (anything else
//! says no). Used before saving into a folder that doesn't exist yet: the
//! question confirms the path and says the folder is new.

use super::*;

pub(super) struct Confirm {
    pub question: String,
    /// Done on `y`.
    action: Answer,
}

enum Answer {
    /// Create the folder, then run the command that needed it.
    CreateFolder { folder: PathBuf, then: Command },
}

impl VimpdfApp {
    /// For a command that writes a new file: if the file's folder doesn't
    /// exist, ask whether to create it and hold the command until then.
    /// True when the command was held.
    pub(super) fn ask_for_missing_folder(&mut self, cmd: &Command) -> bool {
        let target = match cmd {
            Command::SaveAs(p) | Command::Write(Some(p)) | Command::ApplyRedactions(Some(p)) => {
                self.resolve(p.clone())
            }
            _ => return false,
        };
        let Some(folder) = target.parent().filter(|f| !f.as_os_str().is_empty()) else {
            return false;
        };
        if folder.is_dir() {
            return false;
        }
        self.confirm = Some(Confirm {
            question: format!(
                "Folder {} doesn't exist. Create it and save {}? y/n",
                folder.display(),
                target.file_name().unwrap_or_default().to_string_lossy()
            ),
            action: Answer::CreateFolder {
                folder: folder.to_owned(),
                then: cmd.clone(),
            },
        });
        self.message = None;
        true
    }

    /// Keys while a question is asked: `y` does it, anything else cancels.
    pub(super) fn confirm_key(&mut self, ctx: &egui::Context, tok: &str) {
        let Some(confirm) = self.confirm.take() else {
            return;
        };
        if !matches!(tok, "y" | "Y") {
            return self.info("Not saved");
        }
        match confirm.action {
            Answer::CreateFolder { folder, then } => match std::fs::create_dir_all(&folder) {
                Ok(()) => self.execute(then, ctx),
                Err(e) => self.error(format!("cannot create {}: {e}", folder.display())),
            },
        }
    }
}
