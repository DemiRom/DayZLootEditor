use std::{
    collections::{HashMap, HashSet},
    fs,
    io,
    path::PathBuf,
};
use std::iter::Map;
use tui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use xml::{
    reader::{EventReader, XmlEvent},
    writer::EmitterConfig,
};

use crate::action::Action;
use crate::utils;

#[derive(Clone, Debug)]
enum FieldKey {
    Element { name: String, index: usize },
    Attribute { element: String, index: usize, attr: String },
}

#[derive(Clone, Debug)]
struct Field {
    key: FieldKey,
    value: String,
}

#[derive(Clone, Debug)]
struct TypeEntry {
    name: String,
    fields: Vec<Field>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorFocus {
    TypeList,
    FieldList,
    Editing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditTarget {
    TypeName,
    FieldValue,
}

pub struct Editor {
    path: Option<PathBuf>,
    types: Vec<TypeEntry>,
    selected_type: usize,
    selected_field: usize,
    focus: EditorFocus,
    editing_target: Option<EditTarget>,
    input_buffer: String,
    status: String,
}

impl FieldKey {
    pub fn get_field_name(&self) -> &str {
        match self {
            FieldKey::Element { name, .. } => name.as_str(),
            FieldKey::Attribute { attr, .. } => attr.as_str(),
        }
    }

    pub fn get_help_text(&self) -> String{
        match self {
            FieldKey::Element { name, .. } => {
                match name.as_str() {
                    "nominal" => "The nominal (wanted) amount in the server. Same as max is max is not used.".to_string(),
                    "lifetime" => "The amount of time it takes for the item to despawn when the item is on the ground.\nDoes not come into effect if the item is ruined".to_string(),
                    "restock" => "How long after one of the same item (despawns or is picked up by the player) is a new one spawned.".to_string(),
                    "min" => "Minimum quantity of items to spawn this applies to the entire map.".to_string(),
                    "quantmin" => "Minimum quantity of the item to spawn in a stack. Eg. Ammunition stack quantity.".to_string(),
                    "quantmax" => "Maximum quantity of the item to spawn in a stack. Eg. Ammunition stack quantity.".to_string(),
                    "cost" => "Loot spawning prioritizer - no one really knows what this does exactly. :D".to_string(),
                    "category" | "usage" | "tag" => "The location class of where this item can spawn.".to_string(),
                    "flags" => "".to_string(),
                    _ => "Unknown field - open a github issue with the field name.".to_string()
                }
            }
            FieldKey::Attribute { attr, .. } => {
                match attr.as_str() {
                    "count_in_cargo" => "Boolean flag. Sets the total amount that can spawn (map wide) in cargo (tents, boxes, vehicles).\nIf flag is set to 1, item won't spawn if there are already a nominal number of items for this flag.".to_string(),
                    "count_in_hoarder" => "Boolean flag. Sets the total amount that can spawn (map wide) in Zombies.\nIf flag is set to 1, item won't spawn if there are already a nominal number of items for this flag.".to_string(),
                    "count_in_map" => "Boolean flag. Sets the total amount that can spawn on the map.\nIf flag is set to 1, item won't spawn if there are already a nominal number of items for this flag.".to_string(),
                    "count_in_player" => "Boolean flag. Sets the total amount that can spawn (map wide) on players.\nIf flag is set to 1, item won't spawn if there are already a nominal number of items for this flag.".to_string(),
                    "crafted" => "Boolean flag. Sets the total amount based on crafted count (map wide)\nIf flag is set to 1, item won't spawn if there are already a nominal number of items for this flag.".to_string(),
                    "deloot" => "Boolean flag. Sets the total amount (map wide) from dynamic events. E.g Helicopter crashes etc.\nIf flag is set to 1, item won't spawn if there are already a nominal number of items for this flag.".to_string(),
                    "name" => "The location class of where this item can spawn.".to_string(),
                    _ => {
                        format!("Unknown attribute - open a github issue. {}", attr)
                    },
                }
            }
        }
    }

    pub fn get_element_name(&self) -> &str {
        match self {
            FieldKey::Element { name, .. } => name.as_str(),
            FieldKey::Attribute { element, .. } => element.as_str(),
        }
    }
}

impl Editor {
    pub fn new() -> Self {
        Self {
            path: None,
            types: Vec::new(),
            selected_type: 0,
            selected_field: 0,
            focus: EditorFocus::TypeList,
            editing_target: None,
            input_buffer: String::new(),
            status: String::from("Load a file to begin"),
        }
    }

    pub fn load(&mut self, path: PathBuf) -> io::Result<()> {
        let content = fs::read_to_string(&path)?;
        let types = parse_types(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("XML parse error: {}", e)))?;

        self.path = Some(path);
        self.types = types;
        self.selected_type = 0;
        self.selected_field = 0;
        self.focus = EditorFocus::TypeList;
        self.editing_target = None;
        self.input_buffer.clear();
        self.status = String::from("Loaded file");
        Ok(())
    }

    pub fn is_editing(&self) -> bool {
        self.focus == EditorFocus::Editing
    }

    pub fn handle_action(&mut self, action: Action) -> io::Result<()> {
        match self.focus {
            EditorFocus::Editing => {
                match action {
                    Action::Input(c) => self.input_buffer.push(c),
                    Action::Backspace => {
                        self.input_buffer.pop();
                    }
                    Action::Activate => {
                        self.apply_input();
                        self.stop_editing();
                    }
                    Action::Cancel => {
                        self.input_buffer.clear();
                        self.stop_editing();
                        self.status = String::from("Edit cancelled");
                    }
                    _ => {}
                }
            }
            _ => match action {
                Action::Up => self.move_selection(-1),
                Action::Down => self.move_selection(1),
                Action::Left => self.focus = EditorFocus::TypeList,
                Action::Right => {
                    if !self.types.is_empty() {
                        self.focus = EditorFocus::FieldList;
                    }
                }
                Action::PgUp => self.move_selection(-10),
                Action::PgDown => self.move_selection(10),
                Action::Activate => {
                    self.begin_editing();
                }
                Action::Add => self.add(),
                Action::Copy => self.copy(),
                Action::Delete => self.delete(),
                Action::Save => {
                    self.save()?;
                }
                _ => {}
            },
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
                    Constraint::Min(10),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .split(f.size());

        let header_text = match &self.path {
            Some(path) => format!("Editing: {}", path.display()),
            None => String::from("No file loaded"),
        };
        let header = Paragraph::new(header_text)
            .block(Block::default().title("File").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(header, chunks[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(35),
                Constraint::Percentage(45),
                Constraint::Percentage(20)
            ].as_ref())
            .split(chunks[1]);

        let type_items: Vec<ListItem> = self
            .types
            .iter()
            .map(|t| ListItem::new(t.name.clone()))
            .collect();
        let mut type_state = ListState::default();
        if !self.types.is_empty() {
            type_state.select(Some(self.selected_type));
        }
        let type_list = List::new(type_items)
            .block(Block::default().title("Types").borders(Borders::ALL))
            .highlight_symbol("▶ ")
            .highlight_style(highlight_for(self.focus == EditorFocus::TypeList));
        f.render_stateful_widget(type_list, body[0], &mut type_state);

        let field_items: Vec<ListItem> = self
            .current_fields()
            .iter()
            .map(|field| {
                let label = format!("{}: {}", field_label(&field.key), field.value);
                ListItem::new(label)
            })
            .collect();
        let mut field_state = ListState::default();
        if !field_items.is_empty() {
            field_state.select(Some(self.selected_field));
        }
        let field_list = List::new(field_items)
            .block(Block::default().title("Fields").borders(Borders::ALL))
            .highlight_symbol("▶ ")
            .highlight_style(highlight_for(
                self.focus == EditorFocus::FieldList || self.focus == EditorFocus::Editing,
            ));
        f.render_stateful_widget(field_list, body[1], &mut field_state);

        let selected_field_string = self.current_field().unwrap().key.get_help_text().to_string();
        let tips_widget = Paragraph::new(selected_field_string)
            .block(Block::default().title("Tips").borders(Borders::ALL))
            .wrap(Wrap { trim: true });

        f.render_widget(tips_widget, body[2]);

        let footer_text = if self.focus == EditorFocus::Editing {
            format!("Help: ? | Quit: q | Stat//us: editing ({})", self.input_buffer)
        } else {//
            format!("Help: ? | Quit: q | Status: {}", self.status)
        };
        let footer = Paragraph::new(footer_text)
            .block(Block::default().title("Status").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(footer, chunks[2]);

        if show_help {
            render_help_overlay(f);
        }
    }

    fn calculate_move_idx(&mut self, selected: usize, new_idx: isize, arr_len: isize) -> isize {
        if selected == (arr_len - 1) as usize && new_idx >= arr_len {
            return 0;
        } else if selected == 0 && new_idx < 0 {
            return arr_len - 1;
        } else if new_idx < 0 {
            return 0;
        } else if new_idx >= arr_len {
            return arr_len - 1;
        }
        new_idx
    }

    fn move_selection(&mut self, delta: isize) {
        match self.focus {
            EditorFocus::TypeList => {
                if self.types.is_empty() {
                    return;
                }
                let types_len = self.types.len() as isize;
                let new_idx = self
                    .calculate_move_idx(self.selected_type, self.selected_type as isize + delta, types_len);
                self.selected_type = new_idx as usize;
                self.selected_field = 0;
            }
            EditorFocus::FieldList => {
                let fields_len = self.current_fields_len() as isize;
                if fields_len == 0 {
                    return;
                }
                let new_idx = self
                    .calculate_move_idx(self.selected_field, self.selected_field as isize + delta, fields_len);
                self.selected_field = new_idx as usize;
            }
            EditorFocus::Editing => {}
        }
    }

    fn begin_editing(&mut self) {
        match self.focus {
            EditorFocus::TypeList => {
                if let Some(ty) = self.types.get(self.selected_type) {
                    self.input_buffer = ty.name.clone();
                    self.editing_target = Some(EditTarget::TypeName);
                    self.focus = EditorFocus::Editing;
                    self.status = String::from("Editing type name");
                }
            }
            EditorFocus::FieldList => {
                if let Some(field) = self.current_field() {
                    self.input_buffer = field.value.clone();
                    self.editing_target = Some(EditTarget::FieldValue);
                    self.focus = EditorFocus::Editing;
                    self.status = String::from("Editing field");
                }
            }
            EditorFocus::Editing => {}
        }
    }

    fn stop_editing(&mut self) {
        self.focus = match self.editing_target {
            Some(EditTarget::TypeName) => EditorFocus::TypeList,
            Some(EditTarget::FieldValue) => EditorFocus::FieldList,
            None => self.focus,
        };
        self.editing_target = None;
        self.input_buffer.clear();
    }

    fn apply_input(&mut self) {
        let value = self.input_buffer.clone();
        match self.editing_target {
            Some(EditTarget::TypeName) => {
                if let Some(ty) = self.types.get_mut(self.selected_type) {
                    ty.name = value;
                    self.status = String::from("Type renamed");
                }
            }
            Some(EditTarget::FieldValue) => {
                if let Some(field) = self.current_field_mut() {
                    field.value = value;
                    self.status = String::from("Value updated");
                }
            }
            None => {}
        }
    }

    fn save(&mut self) -> io::Result<()> {
        let path = match &self.path {
            Some(p) => p.clone(),
            None => {
                self.status = String::from("No file loaded");
                return Ok(());
            }
        };
        let mut backup_path = path.clone();
        backup_path.add_extension("bak");

        let content = fs::read_to_string(&path)?;
        fs::write(&backup_path, content)?;
        self.status = format!("Created Backup {}", backup_path.display());

        let xml = serialize_types(&self.types)?;
        fs::write(&path, xml)?;
        self.status = format!("Saved {}", path.display());
        Ok(())
    }

    fn add(&mut self) {
        match self.focus {
            EditorFocus::TypeList => {
                let new_type = TypeEntry {
                    name: String::from("new_type"),
                    fields: default_fields(),
                };
                self.types.push(new_type);
                self.selected_type = self.types.len().saturating_sub(1);
                self.selected_field = 0;
                self.focus = EditorFocus::TypeList;
                self.editing_target = Some(EditTarget::TypeName);
                self.input_buffer = String::from("new_type");
                self.status = String::from("Enter a name for the new type");
            }
            EditorFocus::FieldList => {
                if self.types.is_empty() {
                    return;
                }
                let new_field_name = String::from("new_field");
                let idx = self
                    .types
                    .get(self.selected_type)
                    .map(|t| t.fields.iter().filter(|f| matches!(&f.key, FieldKey::Element { name, .. } if name == &new_field_name)).count())
                    .unwrap_or(0);
                let field = Field {
                    key: FieldKey::Element { name: new_field_name.clone(), index: idx },
                    value: String::new(),
                };
                if let Some(ty) = self.types.get_mut(self.selected_type) {
                    ty.fields.push(field);
                    self.selected_field = ty.fields.len().saturating_sub(1);
                    self.begin_editing();
                }
                self.status = String::from("Added new field; enter a value");
            }
            EditorFocus::Editing => {}
        }
    }

    fn copy(&mut self) {
        match self.focus {
            EditorFocus::TypeList => {
                if let Some(current) = self.types.get(self.selected_type).cloned() {
                    let mut clone = current.clone();
                    clone.name = format!("{}_copy", clone.name);
                    self.types.push(clone);
                    self.selected_type = self.types.len().saturating_sub(1);
                    self.selected_field = 0;
                    self.status = String::from("Type copied");
                }
            }
            EditorFocus::FieldList => {
                if let Some(ty) = self.types.get_mut(self.selected_type) {
                    if let Some(field) = ty.fields.get(self.selected_field).cloned() {
                        ty.fields.push(field);
                        self.selected_field = ty.fields.len().saturating_sub(1);
                        self.status = String::from("Field copied");
                    }
                }
            }
            EditorFocus::Editing => {}
        }
    }

    fn delete(&mut self) {
        match self.focus {
            EditorFocus::TypeList => {
                if !self.types.is_empty() {
                    self.types.remove(self.selected_type);
                    if self.selected_type >= self.types.len() && !self.types.is_empty() {
                        self.selected_type = self.types.len() - 1;
                    } else if self.types.is_empty() {
                        self.selected_type = 0;
                    }
                    self.selected_field = 0;
                    self.status = String::from("Type deleted");
                }
            }
            EditorFocus::FieldList => {
                if let Some(ty) = self.types.get_mut(self.selected_type) {
                    if !ty.fields.is_empty() {
                        ty.fields.remove(self.selected_field);
                        if self.selected_field >= ty.fields.len() && !ty.fields.is_empty() {
                            self.selected_field = ty.fields.len() - 1;
                        } else if ty.fields.is_empty() {
                            self.selected_field = 0;
                        }
                        self.status = String::from("Field deleted");
                    }
                }
            }
            EditorFocus::Editing => {}
        }
    }

    fn current_fields(&self) -> Vec<Field> {
        self.types
            .get(self.selected_type)
            .map(|t| t.fields.clone())
            .unwrap_or_default()
    }

    fn current_fields_len(&self) -> usize {
        self.types.get(self.selected_type).map(|t| t.fields.len()).unwrap_or(0)
    }

    fn current_field(&self) -> Option<&Field> {
        self.types
            .get(self.selected_type)
            .and_then(|t| t.fields.get(self.selected_field))
    }

    fn current_field_mut(&mut self) -> Option<&mut Field> {
        self.types
            .get_mut(self.selected_type)
            .and_then(|t| t.fields.get_mut(self.selected_field))
    }
}

fn field_label(key: &FieldKey) -> String {
    match key {
        FieldKey::Element { name, .. } => name.clone(),
        FieldKey::Attribute { element, attr, .. } => format!("{} @{}", element, attr),
    }
}

fn highlight_for(active: bool) -> Style {
    if active {
        Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default()
    }
}

fn render_help_overlay<B: tui::backend::Backend>(f: &mut tui::Frame<B>) {
    let area = utils::centered_rect(70, 70, f.size());
    let text = "Editor Help\n\nNavigation: Up/Down or j/k or PageUp/PageDown to move, Left/Right to switch pane\nEditing: Enter to edit, Esc to cancel, type to change text, Enter to apply\nActions: a add (type or field), c copy, d delete, s save, q quit, ? help";
    let block = Block::default().title("Help").borders(Borders::ALL);
    let help = Paragraph::new(text).wrap(Wrap { trim: true }).block(block);
    f.render_widget(Clear, area);
    f.render_widget(help, area);
}


fn parse_types(content: &str) -> Result<Vec<TypeEntry>, xml::reader::Error> {
    let parser = EventReader::new(content.as_bytes());
    let mut types = Vec::new();
    let mut current: Option<TypeEntry> = None;
    let mut element_indices: HashMap<String, usize> = HashMap::new();
    let mut current_element: Option<(String, usize)> = None;

    for event in parser {
        match event? {
            XmlEvent::StartElement { name, attributes, .. } => {
                let el = name.local_name;
                if el == "type" {
                    let name_attr = attributes
                        .iter()
                        .find(|a| a.name.local_name == "name")
                        .map(|a| a.value.clone())
                        .unwrap_or_default();
                    current = Some(TypeEntry {
                        name: name_attr,
                        fields: Vec::new(),
                    });
                    element_indices.clear();
                    current_element = None;
                } else {
                    let idx = *element_indices.entry(el.clone()).or_insert(0);
                    if let Some(count) = element_indices.get_mut(&el) {
                        *count += 1;
                    }
                    if let Some(ref mut t) = current {
                        for attr in attributes {
                            t.fields.push(Field {
                                key: FieldKey::Attribute {
                                    element: el.clone(),
                                    index: idx,
                                    attr: attr.name.local_name,
                                },
                                value: attr.value,
                            });
                        }
                    }
                    current_element = Some((el, idx));
                }
            }
            XmlEvent::Characters(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let (Some((el, idx)), Some(ref mut t)) = (current_element.clone(), current.as_mut()) {
                    t.fields.push(Field {
                        key: FieldKey::Element { name: el, index: idx },
                        value: trimmed.to_string(),
                    });
                }
            }
            XmlEvent::EndElement { name } => {
                let el = name.local_name;
                if el == "type" {
                    if let Some(t) = current.take() {
                        types.push(t);
                    }
                    element_indices.clear();
                }
                current_element = None;
            }
            _ => {}
        }
    }

    Ok(types)
}

fn serialize_types(types: &[TypeEntry]) -> io::Result<String> {
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut writer = EmitterConfig::new()
            .perform_indent(true)
            .create_writer(&mut buf);

        writer
            .write(xml::writer::XmlEvent::start_element("types"))
            .map_err(to_io)?;

        for t in types {
            let type_element = xml::writer::XmlEvent::start_element("type").attr("name", t.name.as_str());
            writer.write(type_element).map_err(to_io)?;

            let mut order: Vec<(String, usize)> = Vec::new();
            let mut seen: HashSet<(String, usize)> = HashSet::new();
            let mut element_map: HashMap<(String, usize), ElementData> = HashMap::new();

            for field in &t.fields {
                let key = match &field.key {
                    FieldKey::Element { name, index } => (name.clone(), *index),
                    FieldKey::Attribute { element, index, .. } => (element.clone(), *index),
                };
                if seen.insert(key.clone()) {
                    order.push(key.clone());
                }
                let entry = element_map.entry(key).or_insert_with(ElementData::default);
                match &field.key {
                    FieldKey::Element { .. } => entry.text = Some(field.value.clone()),
                    FieldKey::Attribute { attr, .. } => entry.attrs.push((attr.clone(), field.value.clone())),
                }
            }

            for (element, index) in order {
                if let Some(data) = element_map.get(&(element.clone(), index)) {
                    let mut elem = xml::writer::XmlEvent::start_element(element.as_str());
                    for (k, v) in &data.attrs {
                        elem = elem.attr(k.as_str(), v.as_str());
                    }
                    writer.write(elem).map_err(to_io)?;
                    if let Some(text) = &data.text {
                        writer.write(xml::writer::XmlEvent::characters(text)).map_err(to_io)?;
                    }
                    writer.write(xml::writer::XmlEvent::end_element()).map_err(to_io)?;
                }
            }

            writer
                .write(xml::writer::XmlEvent::end_element())
                .map_err(to_io)?;
        }

        writer
            .write(xml::writer::XmlEvent::end_element())
            .map_err(to_io)?;
    }

    let xml_string = String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(xml_string)
}

#[derive(Default)]
struct ElementData {
    attrs: Vec<(String, String)>,
    text: Option<String>,
}

fn to_io<T>(err: T) -> io::Error
where
    T: Into<xml::writer::Error>,
{
    let err: xml::writer::Error = err.into();
    io::Error::new(io::ErrorKind::Other, err)
}

fn default_fields() -> Vec<Field> {
    vec![
        Field {
            key: FieldKey::Element {
                name: "nominal".to_string(),
                index: 0,
            },
            value: String::new(),
        },
        Field {
            key: FieldKey::Element {
                name: "lifetime".to_string(),
                index: 0,
            },
            value: String::new(),
        },
        Field {
            key: FieldKey::Element {
                name: "restock".to_string(),
                index: 0,
            },
            value: String::new(),
        },
        Field {
            key: FieldKey::Element {
                name: "min".to_string(),
                index: 0,
            },
            value: String::new(),
        },
        Field {
            key: FieldKey::Element {
                name: "quantmin".to_string(),
                index: 0,
            },
            value: String::new(),
        },
        Field {
            key: FieldKey::Element {
                name: "quantmax".to_string(),
                index: 0,
            },
            value: String::new(),
        },
        Field {
            key: FieldKey::Element {
                name: "cost".to_string(),
                index: 0,
            },
            value: String::new(),
        },
        Field {
            key: FieldKey::Attribute {
                element: "flags".to_string(),
                index: 0,
                attr: "count_in_cargo".to_string(),
            },
            value: String::from("0"),
        },
        Field {
            key: FieldKey::Attribute {
                element: "flags".to_string(),
                index: 0,
                attr: "count_in_hoarder".to_string(),
            },
            value: String::from("0"),
        },
        Field {
            key: FieldKey::Attribute {
                element: "flags".to_string(),
                index: 0,
                attr: "count_in_map".to_string(),
            },
            value: String::from("1"),
        },
        Field {
            key: FieldKey::Attribute {
                element: "flags".to_string(),
                index: 0,
                attr: "count_in_player".to_string(),
            },
            value: String::from("0"),
        },
        Field {
            key: FieldKey::Attribute {
                element: "flags".to_string(),
                index: 0,
                attr: "crafted".to_string(),
            },
            value: String::from("0"),
        },
        Field {
            key: FieldKey::Attribute {
                element: "flags".to_string(),
                index: 0,
                attr: "deloot".to_string(),
            },
            value: String::from("0"),
        },
        Field {
            key: FieldKey::Attribute {
                element: "category".to_string(),
                index: 0,
                attr: "name".to_string(),
            },
            value: String::new(),
        },
    ]
}
