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

use crate::file_picker::FilePicker;
use crate::editor::Editor;
use crate::action::Action;
use crate::window_state::WindowState;

fn main() -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut file_picker= FilePicker::new(std::env::current_dir()?)?;
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
                    let editing_mode = matches!(state, WindowState::Editor) && editor.is_editing();
                    let action = map_key_to_action(key, editing_mode);
                    match (state, action) {
                        (_, Action::Quit) => running = false,
                        (_, Action::Help) if !editing_mode => {
                            show_help = !show_help;
                        }
                        (WindowState::FilePicker, action) => {
                            if let Some(path_buffer) = file_picker.handle_action(action)? {
                                match editor.load(path_buffer) {
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

fn map_key_to_action(key: KeyEvent, editing: bool) -> Action {
    if editing {
        return match key.code {
            KeyCode::Enter => Action::Activate,
            KeyCode::Esc => Action::Cancel,
            KeyCode::Backspace => Action::Backspace,
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
        KeyCode::Char('?') => Action::Help,
        KeyCode::Esc => Action::Cancel,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Char(c) => Action::Input(c),
        KeyCode::PageUp => Action::PgUp,
        KeyCode::PageDown => Action::PgDown,
        _ => Action::None,
    }
}
