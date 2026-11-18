//! File and folder dialogs behind one seam, so tests can script the answers
//! a user would give instead of opening native Windows dialogs.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub type DialogFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Debug, Clone, Default)]
pub enum Dialogs {
    /// The operating system's dialogs, through rfd.
    #[default]
    Native,
    /// Answers queued by a test, used in order. An empty answer is a
    /// cancelled dialog.
    #[cfg(test)]
    Scripted(std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Vec<PathBuf>>>>),
}

impl Dialogs {
    /// Asks for one or more `.xml` files to open. Empty when cancelled.
    pub fn pick_files(&self, dir: PathBuf) -> DialogFuture<Vec<PathBuf>> {
        match self {
            Dialogs::Native => Box::pin(async move {
                rfd::AsyncFileDialog::new()
                    .add_filter("Encounter", &["xml"])
                    .set_directory(dir)
                    .pick_files()
                    .await
                    .map(|hs| hs.iter().map(|h| h.path().to_path_buf()).collect())
                    .unwrap_or_default()
            }),
            #[cfg(test)]
            Dialogs::Scripted(_) => {
                let answer = self.next_answer();
                Box::pin(async move { answer })
            }
        }
    }

    /// Asks where to save an `.xml` file.
    pub fn save_file(&self, dir: PathBuf, name: String) -> DialogFuture<Option<PathBuf>> {
        match self {
            Dialogs::Native => Box::pin(async move {
                rfd::AsyncFileDialog::new()
                    .add_filter("Encounter", &["xml"])
                    .set_directory(dir)
                    .set_file_name(name)
                    .save_file()
                    .await
                    .map(|h| h.path().to_path_buf())
            }),
            #[cfg(test)]
            Dialogs::Scripted(_) => {
                let answer = self.next_answer().into_iter().next();
                Box::pin(async move { answer })
            }
        }
    }

    /// Asks for a folder.
    pub fn pick_folder(&self, start: PathBuf) -> DialogFuture<Option<PathBuf>> {
        match self {
            Dialogs::Native => Box::pin(async move {
                rfd::AsyncFileDialog::new()
                    .set_directory(start)
                    .pick_folder()
                    .await
                    .map(|h| h.path().to_path_buf())
            }),
            #[cfg(test)]
            Dialogs::Scripted(_) => {
                let answer = self.next_answer().into_iter().next();
                Box::pin(async move { answer })
            }
        }
    }
}

#[cfg(test)]
impl Dialogs {
    pub fn scripted() -> Self {
        Dialogs::Scripted(Default::default())
    }

    /// Queues the answer for the next dialog; an empty list cancels it.
    pub fn answer(&self, paths: Vec<PathBuf>) {
        if let Dialogs::Scripted(queue) = self {
            queue.lock().unwrap().push_back(paths);
        }
    }

    fn next_answer(&self) -> Vec<PathBuf> {
        match self {
            Dialogs::Scripted(queue) => queue
                .lock()
                .unwrap()
                .pop_front()
                .expect("a dialog opened with no scripted answer queued"),
            Dialogs::Native => unreachable!(),
        }
    }
}
