use std::{fs, io, path::PathBuf};

use crate::action::Action;
use tui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::utils;

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    is_dir: bool,
}

#[derive(Default)]
pub struct FilePicker {
    cwd: PathBuf,
    entries: Vec<Entry>,
    state: ListState,
    status: String,
    selected: PathBuf
}

impl FilePicker {
    pub fn new(cwd: PathBuf) -> io::Result<Self> {
        let mut picker = Self {
            cwd,
            entries: Vec::new(),
            state: ListState::default(),
            status: String::from("Press Enter to open, q to quit"),
            selected: PathBuf::new(),
        };

        picker.refresh_entries()?;
        Ok(picker)
    }

    pub fn get_path(&mut self) -> Result<PathBuf, ()> {
        if self.selected.is_file() {
            return Ok(self.selected.clone());
        }
        Err(())
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    pub fn handle_action(&mut self, action: Action) -> io::Result<Option<PathBuf>> {
        match action {
            Action::Up => self.previous(),
            Action::Down => self.next(),
            Action::Activate => {
                self.enter_directory_or_select_file()?;
                if let Ok(path) = self.get_path() {
                    return Ok(Some(path));
                }
            }
            _ => {}
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

    pub fn enter_directory_or_select_file(&mut self) -> io::Result<()> {
        let Some(idx) = self.state.selected() else {
            return Ok(());
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
        } else {
            let selected = self.cwd.join(&entry.name);
            self.selected = selected.clone();
            self.status = format!("Selected file: {}", selected.display());
        }
        Ok(())
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
            "Help: ? | Quit: q | Status: {}",
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
    }
}

fn render_help_overlay<B: tui::backend::Backend>(f: &mut tui::Frame<B>) {
    let area = utils::centered_rect(70, 70, f.size());
    let text = "File Picker Help\n\n- Up/Down or j/k: move\n- Enter: open directory/select file\n- q: quit\n- ?: toggle this help";
    let block = Block::default().title("Help").borders(Borders::ALL);
    let help = Paragraph::new(text).wrap(Wrap { trim: true }).block(block);
    f.render_widget(Clear, area);
    f.render_widget(help, area);
}
