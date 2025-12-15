use std::{io, time::Duration};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use tui::{
    backend::CrosstermBackend,
    Terminal,
};

mod file_picker;
mod editor;
mod action;
mod window_state;
mod utils;
mod remote;

use crate::file_picker::FilePicker;
use crate::editor::Editor;
use crate::action::Action;
use crate::window_state::WindowState;
use crate::remote::RemoteConfig;

fn main() -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let remote_config = RemoteConfig::from_env();
    let mut file_picker= FilePicker::new(std::env::current_dir()?, remote_config)?;
    let mut editor = Editor::new();

    let mut state = WindowState::FilePicker;
    let mut show_help = false;

    let mut running = true;
    while running {
        match state {
            WindowState::FilePicker => {
                let help = show_help;
                terminal.draw(|f| file_picker.draw(f, help))?;
            },
            WindowState::Editor => {
                let help = show_help;
                terminal.draw(|f| editor.draw(f, help))?;
            }
        }

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => {
                    let text_editing = matches!(state, WindowState::Editor) && editor.is_editing();
                    let prompt_mode = matches!(state, WindowState::FilePicker) && file_picker.is_prompt();
                    let action = map_key_to_action(key, text_editing, prompt_mode);
                    match (state, action) {
                        (_, Action::Quit) => running = false,
                        (_, Action::Help) if !text_editing && !prompt_mode => {
                            show_help = !show_help;
                        }
                        (WindowState::FilePicker, action) => {
                            if let Some(selection) = file_picker.handle_action(action)? {
                                match editor.load(selection) {
                                    Ok(_) => {
                                        state = WindowState::Editor;
                                    }
                                    Err(err) => {
                                        file_picker.set_status(format!("Failed to open file: {}", err));
                                    }
                                }
                            }
                        }
                        (WindowState::Editor, action) => {
                            editor.handle_action(action)?;
                        }
                    }
                }
                Event::Resize(_, _) => {
                    // Let the next draw handle the new size.
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn map_key_to_action(key: KeyEvent, text_editing: bool, prompt_mode: bool) -> Action {
    if text_editing {
        return match key.code {
            KeyCode::Enter => Action::Activate,
            KeyCode::Esc => Action::Cancel,
            KeyCode::Backspace => Action::Backspace,
            KeyCode::Char(c) => Action::Input(c),
            _ => Action::None,
        };
    }

    if prompt_mode {
        return match key.code {
            KeyCode::Enter => Action::Activate,
            KeyCode::Esc => Action::Cancel,
            KeyCode::Backspace => Action::Backspace,
            KeyCode::Tab => Action::Tab,
            KeyCode::Up => Action::Up,
            KeyCode::Down => Action::Down,
            KeyCode::PageUp => Action::PgUp,
            KeyCode::PageDown => Action::PgDown,
            KeyCode::Char(c) => Action::Input(c),
            _ => Action::None,
        };
    }

    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Up | KeyCode::Char('k') => Action::Up,
        KeyCode::Down | KeyCode::Char('j') => Action::Down,
        KeyCode::Enter => Action::Activate,
        KeyCode::Left | KeyCode::Char('h') => Action::Left,
        KeyCode::Right | KeyCode::Char('l') => Action::Right,
        KeyCode::Char('s') => Action::Save,
        KeyCode::Char('a') => Action::Add,
        KeyCode::Char('c') => Action::Copy,
        KeyCode::Char('d') => Action::Delete,
        KeyCode::Char('t') => Action::AddAttribute,
        KeyCode::Char('r') => Action::ToggleRemote,
        KeyCode::Char('?') => Action::Help,
        KeyCode::Tab => Action::Tab,
        KeyCode::Esc => Action::Cancel,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Char(c) => Action::Input(c),
        KeyCode::PageUp => Action::PgUp,
        KeyCode::PageDown => Action::PgDown,
        _ => Action::None,
    }
}
