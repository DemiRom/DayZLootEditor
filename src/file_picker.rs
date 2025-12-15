use std::{fs, io, path::PathBuf, sync::{Arc, Mutex}};

use crate::{action::Action, remote::{DirEntry, FileSelection, FileSource, RemoteConfig, SshBackend}, utils};
use tui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    is_dir: bool,
}

pub struct FilePicker {
    local_root: PathBuf,
    cwd: PathBuf,
    entries: Vec<Entry>,
    state: ListState,
    status: String,
    backend: PickerBackend,
    remote_config: Option<RemoteConfig>,
    mode: PickerMode,
}

enum PickerBackend {
    Local,
    Remote(Arc<Mutex<SshBackend>>),
}

enum PickerMode {
    Browse,
    RemotePrompt(RemoteForm),
}

struct RemoteForm {
    host: String,
    user: String,
    port: String,
    password: String,
    key_path: String,
    passphrase: String,
    field_index: usize,
}

impl RemoteForm {
    fn next_field(&mut self) {
        self.field_index = (self.field_index + 1) % 6;
    }

    fn prev_field(&mut self) {
        if self.field_index == 0 {
            self.field_index = 5;
        } else {
            self.field_index -= 1;
        }
    }

    fn active_mut(&mut self) -> &mut String {
        match self.field_index {
            0 => &mut self.host,
            1 => &mut self.user,
            2 => &mut self.port,
            3 => &mut self.password,
            4 => &mut self.key_path,
            5 => &mut self.passphrase,
            _ => &mut self.host,
        }
    }

    fn push_char(&mut self, c: char) {
        self.active_mut().push(c);
    }

    fn pop_char(&mut self) {
        self.active_mut().pop();
    }

    fn to_config(&self) -> RemoteConfig {
        let port: u16 = self.port.parse().unwrap_or(22);
        RemoteConfig {
            host: self.host.clone(),
            port,
            username: self.user.clone(),
            password: if self.password.is_empty() { None } else { Some(self.password.clone()) },
            key_path: if self.key_path.is_empty() { None } else { Some(PathBuf::from(self.key_path.clone())) },
            passphrase: if self.passphrase.is_empty() { None } else { Some(self.passphrase.clone()) },
        }
    }
}

impl FilePicker {
    pub fn new(cwd: PathBuf, remote_config: Option<RemoteConfig>) -> io::Result<Self> {
        let mut picker = Self {
            local_root: cwd.clone(),
            cwd,
            entries: Vec::new(),
            state: ListState::default(),
            status: String::from("Press Enter to open, q to quit"),
            backend: PickerBackend::Local,
            remote_config,
            mode: PickerMode::Browse,
        };

        picker.refresh_entries()?;
        Ok(picker)
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    pub fn handle_action(&mut self, action: Action) -> io::Result<Option<FileSelection>> {
        match &mut self.mode {
            PickerMode::Browse => {
                match action {
                    Action::Up => self.previous(),
                    Action::Down => self.next(),
                    Action::PgUp => self.jump(-5),
                    Action::PgDown => self.jump(5),
                    Action::Activate => {
                        if let Some(selection) = self.enter_directory_or_select_file()? {
                            return Ok(Some(selection));
                        }
                    }
                    Action::ToggleRemote => {
                        match self.backend {
                            PickerBackend::Remote(_) => {
                                self.backend = PickerBackend::Local;
                                self.cwd = self.local_root.clone();
                                self.status = "Switched to local".to_string();
                                self.refresh_entries()?;
                            }
                            PickerBackend::Local => self.start_remote_prompt(),
                        }
                    }
                    _ => {}
                }
            }
            PickerMode::RemotePrompt(form) => {
                match action {
                    Action::Up => form.prev_field(),
                    Action::Down => form.next_field(),
                    Action::Tab => form.next_field(),
                    Action::PgUp => form.prev_field(),
                    Action::PgDown => form.next_field(),
                    Action::Input(c) => form.push_char(c),
                    Action::Backspace => form.pop_char(),
                    Action::Activate => {
                        let cfg = form.to_config();
                        match self.try_connect(cfg) {
                            Ok(true) => {
                                self.mode = PickerMode::Browse;
                                self.refresh_entries()?;
                            }
                            Ok(false) => {}
                            Err(err) => {
                                self.status = format!("SSH connect failed: {err}");
                                self.mode = PickerMode::Browse;
                            }
                        }
                    }
                    Action::Cancel => {
                        self.status = "SSH connect cancelled".to_string();
                        self.mode = PickerMode::Browse;
                    }
                    _ => {}
                }
            }
        }
        Ok(None)
    }

    fn refresh_entries(&mut self) -> io::Result<()> {
        let mut entries = Vec::new();

        if self.cwd.parent().is_some() {
            entries.push(Entry {
                name: "..".to_string(),
                is_dir: true,
            });
        }

        match &self.backend {
            PickerBackend::Local => {
                for entry in fs::read_dir(&self.cwd)? {
                    let entry = entry?;
                    let path = entry.path();
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default()
                        .to_string();
                    let is_dir = path.is_dir();
                    entries.push(Entry { name, is_dir });
                }
            }
            PickerBackend::Remote(remote) => {
                let remote =
                    remote.lock().map_err(|_| io::Error::new(io::ErrorKind::Other, "SSH backend in use"))?;
                let remote_entries = remote.list_dir(&self.cwd)?;
                for DirEntry { name, is_dir } in remote_entries {
                    entries.push(Entry { name, is_dir });
                }
            }
        }

        // Keep directories first, then files, both alphabetically.
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        self.entries = entries;
        self.state.select(Some(0));
        Ok(())
    }

    pub fn next(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) if i + 1 < self.entries.len() => i + 1,
            _ => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) if i > 0 => i - 1,
            _ => self.entries.len() - 1,
        };
        self.state.select(Some(i));
    }

    pub fn enter_directory_or_select_file(&mut self) -> io::Result<Option<FileSelection>> {
        let Some(idx) = self.state.selected() else {
            return Ok(None);
        };
        let entry = &self.entries[idx];

        if entry.is_dir {
            let new_path = if entry.name == ".." {
                self.cwd.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| self.cwd.clone())
            } else {
                self.cwd.join(&entry.name)
            };
            self.cwd = new_path;
            self.status.clear();
            self.refresh_entries()?;
            Ok(None)
        } else {
            let selected = self.cwd.join(&entry.name);
            self.status = format!("Selected file: {} ({})", selected.display(), self.backend_label());
            Ok(Some(FileSelection {
                path: selected,
                source: self.current_source(),
            }))
        }
    }

    fn jump(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let len = self.entries.len() as isize;
        let current = self.state.selected().unwrap_or(0) as isize;
        let mut new = current + delta;
        if new < 0 {
            new = 0;
        } else if new >= len {
            new = len - 1;
        }
        self.state.select(Some(new as usize));
    }

    pub fn draw<B: tui::backend::Backend>(&mut self, f: &mut tui::Frame<B>, show_help: bool) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .split(f.size());

        let location = Paragraph::new(format!("Current directory: {}", self.cwd.display()))
            .block(Block::default().title("Location").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(location, chunks[0]);

        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|entry| {
                let mut label = entry.name.clone();
                if entry.is_dir && entry.name != ".." {
                    label.push('/');
                }
                ListItem::new(label)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title("File Picker").borders(Borders::ALL))
            .highlight_symbol("▶ ")
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));
        f.render_stateful_widget(list, chunks[1], &mut self.state);

        let footer_text = format!(
            "Help: ? | Remote: r | Quit: q | Source: {} | Status: {}",
            self.backend_label(),
            if self.status.is_empty() {
                "No file selected"
            } else {
                &self.status
            }
        );
        let footer = Paragraph::new(footer_text)
            .block(Block::default().borders(Borders::ALL).title("Status"))
            .wrap(Wrap { trim: true });
        f.render_widget(footer, chunks[2]);

        if show_help {
            render_help_overlay(f);
        }
        if let PickerMode::RemotePrompt(form) = &self.mode {
            render_remote_prompt(f, form);
        }
    }

    fn start_remote_prompt(&mut self) {
        let defaults = self.remote_config.clone().unwrap_or_else(|| RemoteConfig {
            host: String::new(),
            port: 22,
            username: std::env::var("USER").unwrap_or_default(),
            password: None,
            key_path: None,
            passphrase: None,
        });
        self.mode = PickerMode::RemotePrompt(RemoteForm {
            host: defaults.host,
            user: defaults.username,
            port: defaults.port.to_string(),
            password: defaults.password.unwrap_or_default(),
            key_path: defaults.key_path.unwrap_or_default().to_string_lossy().to_string(),
            passphrase: defaults.passphrase.unwrap_or_default(),
            field_index: 0,
        });
    }

    fn backend_label(&self) -> &'static str {
        match self.backend {
            PickerBackend::Local => "local",
            PickerBackend::Remote(_) => "ssh",
        }
    }

    fn current_source(&self) -> FileSource {
        match &self.backend {
            PickerBackend::Local => FileSource::Local,
            PickerBackend::Remote(client) => FileSource::Remote(client.clone()),
        }
    }

    fn try_connect(&mut self, cfg: RemoteConfig) -> io::Result<bool> {
        match SshBackend::connect(&cfg) {
            Ok(client) => {
                self.backend = PickerBackend::Remote(client);
                self.cwd = PathBuf::from("/");
                self.status = "Connected via SSH".to_string();
                self.remote_config = Some(cfg);
                Ok(true)
            }
            Err(err) => {
                self.status = format!("SSH connect failed: {err}");
                Ok(false)
            }
        }
    }

    pub fn is_prompt(&self) -> bool {
        matches!(self.mode, PickerMode::RemotePrompt(_))
    }
}

fn render_help_overlay<B: tui::backend::Backend>(f: &mut tui::Frame<B>) {
    let area = utils::centered_rect(70, 70, f.size());
    let text = "File Picker Help\n\n- Up/Down or j/k: move\n- PgUp/PgDown: jump lists\n- Enter: open directory/select file\n- r: toggle SSH (enter host/user/port/password/key); r again returns to local\n- q: quit\n- ?: toggle this help";
    let block = Block::default().title("Help").borders(Borders::ALL);
    let help = Paragraph::new(text).wrap(Wrap { trim: true }).block(block);
    f.render_widget(Clear, area);
    f.render_widget(help, area);
}

fn render_remote_prompt<B: tui::backend::Backend>(f: &mut tui::Frame<B>, form: &RemoteForm) {
    let area = utils::centered_rect(70, 70, f.size());
    let fields = [
        ("Host", &form.host),
        ("User", &form.user),
        ("Port", &form.port),
        ("Password (optional)", &form.password),
        ("Key Path (optional)", &form.key_path),
        ("Passphrase (optional)", &form.passphrase),
    ];
    let lines: Vec<String> = fields
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let marker = if i == form.field_index { ">" } else { " " };
            format!("{marker} {label}: {value}")
        })
        .collect();
    let text = format!(
        "Connect via SSH\nEnter details (leave password empty if using keys)\n\n{}\n\nEnter to connect, Esc to cancel",
        lines.join("\n")
    );
    let block = Block::default().title("SSH Connect").borders(Borders::ALL);
    let prompt = Paragraph::new(text).wrap(Wrap { trim: false }).block(block);
    f.render_widget(Clear, area);
    f.render_widget(prompt, area);
}
