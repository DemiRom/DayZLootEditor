use tui::layout::Rect;
use crate::action::Action;

pub struct Menu {

}

impl Menu {
    pub fn new() -> Self {
        Menu {}
    }

    pub fn handle_action(&mut self, action: Action) {

    }

    pub fn draw<B: tui::backend::Backend>(&mut self, f: &mut tui::Frame<B>, show_help: bool) {

    }
}