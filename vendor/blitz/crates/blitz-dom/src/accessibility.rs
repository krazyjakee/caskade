use crate::{
    BaseDocument, ElementData, Node as BlitzDomNode, NodeData, NodeId as BlitzDomNodeId, local_name,
};
use accesskit::{
    Action, Affine, AriaCurrent, HasPopup, Invalid, Live, Node as AccessKitNode, NodeId,
    Orientation, Rect, Role, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use style::properties::longhands::visibility;
use stylo_taffy::CopyStyleGetters;

impl BaseDocument {
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        let mut nodes = std::collections::HashMap::new();
        let mut window = AccessKitNode::new(Role::Window);
        let mut hidden_nodes = std::collections::HashSet::new();
        let mut labelled_by_nodes = Vec::new();
        let mut described_by_nodes = Vec::new();
        let mut id_ref_nodes = Vec::new();
        let mut labels = Vec::new();

        self.visit(|node_id, node| {
            if node.is_hidden_from_accessibility_tree()
                || node
                    .parent
                    .map(|p| hidden_nodes.contains(&p))
                    .unwrap_or(false)
            {
                hidden_nodes.insert(node_id);
                return;
            }
            let parent = node
                .parent
                .and_then(|parent_id| nodes.get_mut(&parent_id))
                .map(|(_, parent)| parent)
                .unwrap_or(&mut window);
            let (id, builder) = self.build_accessibility_node(node, parent);

            if node.attr(local_name!("aria-labelledby")).is_some() {
                labelled_by_nodes.push(node_id);
            }
            if node.attr(local_name!("aria-describedby")).is_some() {
                described_by_nodes.push(node_id);
            }
            if node.data.is_element_with_tag_name(&local_name!("label")) {
                labels.push(node_id);
            }
            if node.attr(local_name!("aria-activedescendant")).is_some()
                || node.attr(local_name!("aria-controls")).is_some()
            {
                id_ref_nodes.push(node_id);
            }
            nodes.insert(node_id, (id, builder));
        });

        let mut label = FlatText::default();
        for node_id in labelled_by_nodes {
            let Some(id_refs) = self.nodes[node_id].attr(local_name!("aria-labelledby")) else {
                continue;
            };
            let Some((id, mut builder)) = nodes.remove(&node_id) else {
                continue;
            };
            let mut has_target = false;
            label.clear();
            for target in self.referenced_dom_ids_to_node_ids(id_refs) {
                if !has_target {
                    builder.clear_labelled_by();
                    has_target = true;
                }
                if target == node_id || nodes.contains_key(&target) {
                    builder.push_labelled_by(NodeId(target.as_u64()));
                }
                self.nodes[target].write_text_alternative(&mut label);
                label.break_word();
            }
            if !label.text.is_empty() {
                builder.set_label(label.text.as_str());
            }
            nodes.insert(node_id, (id, builder));
        }

        for node_id in described_by_nodes {
            let Some(id_refs) = self.nodes[node_id].attr(local_name!("aria-describedby")) else {
                continue;
            };
            let Some((id, mut builder)) = nodes.remove(&node_id) else {
                continue;
            };
            for target in self.referenced_dom_ids_to_node_ids(id_refs) {
                if target == node_id || nodes.contains_key(&target) {
                    builder.push_described_by(NodeId(target.as_u64()));
                }
            }
            nodes.insert(node_id, (id, builder));
        }

        // A form control without `aria-label` or `aria-labelledby` is named
        // by its labels: `<label for>` or the `<label>` around it
        // (https://www.w3.org/TR/html-aam-1.0/#input-type-text-input-type-password-input-type-number-input-type-search-input-type-tel-input-type-email-input-type-url-and-textarea-element-accessible-name-computation).
        let mut control_labels: Vec<(BlitzDomNodeId, FlatText)> = Vec::new();
        for label_id in labels {
            let Some(control) = self.labelled_control(label_id) else {
                continue;
            };
            let control_node = &self.nodes[control];
            if control_node.attr(local_name!("aria-label")).is_some()
                || control_node.attr(local_name!("aria-labelledby")).is_some()
            {
                continue;
            }
            let index = match control_labels.iter().position(|(id, _)| *id == control) {
                Some(index) => index,
                None => {
                    control_labels.push((control, FlatText::default()));
                    control_labels.len() - 1
                }
            };
            let text = &mut control_labels[index].1;
            text.break_word();
            self.nodes[label_id].write_flat_text_content_except(control, text);
        }
        for (control, text) in control_labels {
            if let Some((_, builder)) = nodes.get_mut(&control)
                && !text.text.is_empty()
            {
                builder.set_label(text.text.as_str());
            }
        }

        // `aria-activedescendant` and `aria-controls` name other elements by
        // id: like the labels above, they need every node built first.
        for node_id in id_ref_nodes {
            let Some((id, mut builder)) = nodes.remove(&node_id) else {
                continue;
            };
            let node = &self.nodes[node_id];
            if let Some(target) = node
                .attr(local_name!("aria-activedescendant"))
                .and_then(|id_ref| self.referenced_dom_ids_to_node_ids(id_ref).next())
                .filter(|target| nodes.contains_key(target))
            {
                builder.set_active_descendant(NodeId(target.as_u64()));
            }
            if let Some(id_refs) = node.attr(local_name!("aria-controls")) {
                for target in self.referenced_dom_ids_to_node_ids(id_refs) {
                    if target == node_id || nodes.contains_key(&target) {
                        builder.push_controlled(NodeId(target.as_u64()));
                    }
                }
            }
            nodes.insert(node_id, (id, builder));
        }

        let mut nodes: Vec<_> = nodes
            .into_iter()
            .map(|(_, (id, node))| (id, node))
            .collect();
        // Bounds are in CSS pixels, relative to the viewport: the window's
        // transform scales them to the window's physical pixels.
        let scale = self.viewport.scale_f64();
        let (width, height) = self.viewport.window_size;
        window.set_transform(Affine::scale(scale));
        window.set_bounds(Rect::new(
            0.0,
            0.0,
            width as f64 / scale,
            height as f64 / scale,
        ));
        nodes.push((NodeId(u64::MAX), window));

        let tree = TreeInfo::new(NodeId(u64::MAX));
        TreeUpdate {
            tree_id: TreeId::ROOT,
            nodes,
            tree: Some(tree),
            focus: NodeId(self.focus_node_id.map(|id| id.as_u64()).unwrap_or(u64::MAX)),
        }
    }

    /// The form control a `<label>` labels: the element its `for` names, or
    /// the first labelable element inside it.
    fn labelled_control(&self, label_id: BlitzDomNodeId) -> Option<BlitzDomNodeId> {
        let is_labelable = |id: BlitzDomNodeId| {
            self.nodes[id].element_data().is_some_and(|el| {
                matches!(
                    &*el.name.local,
                    "button" | "input" | "meter" | "output" | "progress" | "select" | "textarea"
                ) && !(el.name.local == local_name!("input")
                    && el.attr(local_name!("type")) == Some("hidden"))
            })
        };
        match self.nodes[label_id].attr(local_name!("for")) {
            Some(for_id) => self
                .referenced_dom_ids_to_node_ids(for_id.trim_ascii())
                .next()
                .filter(|id| is_labelable(*id)),
            None => {
                let mut stack: Vec<_> = self.nodes[label_id]
                    .children
                    .iter()
                    .rev()
                    .copied()
                    .collect();
                while let Some(id) = stack.pop() {
                    if is_labelable(id) {
                        return Some(id);
                    }
                    stack.extend(self.nodes[id].children.iter().rev().copied());
                }
                None
            }
        }
    }

    fn referenced_dom_ids_to_node_ids<'a>(
        &'a self,
        nodes_reference: &'a str,
    ) -> impl Iterator<Item = BlitzDomNodeId> + 'a {
        nodes_reference
            .split_ascii_whitespace()
            .filter_map(|dom_id| self.nodes_to_id.get(dom_id)?.first().copied())
    }

    fn build_accessibility_node(
        &self,
        node: &BlitzDomNode,
        parent: &mut AccessKitNode,
    ) -> (NodeId, AccessKitNode) {
        let id = NodeId(node.id.as_u64());

        let mut builder = AccessKitNode::default();
        if node.parent.is_none() {
            builder.set_role(Role::Window)
        } else if let Some(element_data) = node.element_data() {
            let name = element_data.name.local.to_string();
            let role_attr = element_data.attr(local_name!("role"));

            // TODO: The roles of elements with strong native semantics cannot be overridden; see
            // https://www.w3.org/TR/wai-aria-1.2/#host_general_conflict.
            // `role` is a list of tokens: the first one known wins.
            let role = role_attr
                .and_then(|roles| roles.split_ascii_whitespace().find_map(role_from_name))
                .or_else(|| role_from_element_data(element_data))
                .unwrap_or(Role::Unknown);

            builder.set_role(role);

            // https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion
            if element_data.attr(local_name!("aria-hidden")) == Some("true") {
                builder.set_hidden();
            }

            if let Some(aria_expanded) = element_data.attr(local_name!("aria-expanded"))
                && let Ok(aria_expanded) = aria_expanded.parse()
            {
                builder.set_expanded(aria_expanded);
            }

            if element_data.attr(local_name!("aria-disabled")) == Some("true")
                || (is_form_control(&name) && element_data.attr(local_name!("disabled")).is_some())
            {
                builder.set_disabled();
            }

            if let Some(aria_selected) = element_data.attr(local_name!("aria-selected"))
                && let Ok(aria_selected) = aria_selected.parse()
            {
                builder.set_selected(aria_selected);
            }

            if let Some(aria_label) = element_data.attr(local_name!("aria-label")) {
                builder.set_label(aria_label);
            }
            if let Some(aria_description) = element_data.attr(local_name!("aria-description")) {
                builder.set_description(aria_description);
            }
            if let Some(aria_level) = element_data.attr(local_name!("aria-level"))
                && let Ok(aria_level) = aria_level.parse::<usize>()
                // Accesskit levels are 0-based, while ARIA levels are 1-based
                // See https://docs.rs/accesskit/latest/accesskit/struct.Node.html#method.level
                && let Some(aria_level) = aria_level.checked_sub(1)
            {
                builder.set_level(aria_level);
            } else if let Some(default_level) = default_level_for_tag(&name) {
                builder.set_level(default_level);
            } else if let Some(default_level) = default_level_for_role(role) {
                builder.set_level(default_level);
            }

            set_states_and_properties(&mut builder, element_data, &name, role);
            add_actions(&mut builder, node, element_data, role);

            if let Some(rect) = self.get_client_bounding_rect(node.id) {
                builder.set_bounds(Rect::new(
                    rect.x,
                    rect.y,
                    rect.x + rect.width,
                    rect.y + rect.height,
                ));
            }

            builder.set_html_tag(name);
        } else if node.is_text_node() {
            // Static text is a label whose value is the text: AccessKit takes
            // a name from a `labelled_by` target's value only for labels.
            // (`TextRun` is for runs inside text fields, with character data.)
            builder.set_role(Role::Label);
            builder.set_value(node.text_content());
            // Only some roles take their name from their content
            // (https://www.w3.org/TR/wai-aria-1.2/#namefromcontent): a status
            // or a generic container read their text, they aren't named by it.
            if names_from_content(parent.role()) {
                parent.push_labelled_by(id)
            }
        }

        parent.push_child(id);

        (id, builder)
    }
}

impl BlitzDomNode {
    // https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion
    fn is_hidden_from_accessibility_tree(&self) -> bool {
        self.try_stylo_element_data()
            .as_ref()
            .and_then(|s| s.get())
            .and_then(|s| {
                let style = s.styles.get_primary()?;
                Some(
                    style.get_box().get_display().is_none()
                        || style.clone_visibility() == visibility::computed_value::T::Hidden,
                )
            })
            .unwrap_or(false)
    }

    fn write_text_alternative(&self, out: &mut FlatText) {
        match self
            .attr(local_name!("aria-label"))
            .filter(|aria_label| !aria_label.trim_ascii().is_empty())
        {
            Some(aria_label) => out.push(aria_label),
            None => self.write_flat_text_content(out),
        }
    }

    fn write_flat_text_content(&self, out: &mut FlatText) {
        self.write_flat_text_content_except(BlitzDomNodeId::default(), out);
    }

    /// The text content, leaving out the subtree of `except` (a labelled
    /// control inside its label).
    fn write_flat_text_content_except(&self, except: BlitzDomNodeId, out: &mut FlatText) {
        if self.id == except {
            return;
        }
        match &self.data {
            NodeData::Text(data) => out.push(&data.content),
            NodeData::Element(..) | NodeData::AnonymousBlock(..) => {
                for &child_id in &self.children {
                    self.with(child_id)
                        .write_flat_text_content_except(except, out);
                }
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct FlatText {
    text: String,
    pending_space: bool,
}

impl FlatText {
    fn push(&mut self, text: &str) {
        for (i, word) in text.split(|c: char| c.is_ascii_whitespace()).enumerate() {
            self.pending_space |= i > 0;
            if word.is_empty() {
                continue;
            }
            if self.pending_space && !self.text.is_empty() {
                self.text.push(' ');
            }
            self.pending_space = false;
            self.text.push_str(word);
        }
    }

    fn break_word(&mut self) {
        self.pending_space = true;
    }

    fn clear(&mut self) {
        self.text.clear();
        self.pending_space = false;
    }
}

fn role_from_name(name: &str) -> Option<Role> {
    match name {
        "alert" => Some(Role::Alert),
        "alertdialog" => Some(Role::AlertDialog),
        "button" => Some(Role::Button),
        "checkbox" => Some(Role::CheckBox),
        "dialog" => Some(Role::Dialog),
        "gridcell" => Some(Role::GridCell),
        "link" => Some(Role::Link),
        "log" => Some(Role::Log),
        "marquee" => Some(Role::Marquee),
        "menuitem" => Some(Role::MenuItem),
        "menuitemcheckbox" => Some(Role::MenuItemCheckBox),
        "menuitemradio" => Some(Role::MenuItemRadio),
        "option" => Some(Role::ListBoxOption),
        "progressbar" => Some(Role::ProgressIndicator),
        "radio" => Some(Role::RadioButton),
        "scrollbar" => Some(Role::ScrollBar),
        "slider" => Some(Role::Slider),
        "spinbutton" => Some(Role::SpinButton),
        "status" => Some(Role::Status),
        "tab" => Some(Role::Tab),
        "tabpanel" => Some(Role::TabPanel),
        "textbox" => Some(Role::TextInput),
        "timer" => Some(Role::Timer),
        "tooltip" => Some(Role::Tooltip),
        "treeitem" => Some(Role::TreeItem),
        "combobox" => Some(Role::ComboBox),
        "grid" => Some(Role::Grid),
        "listbox" => Some(Role::ListBox),
        "menu" => Some(Role::Menu),
        "menubar" => Some(Role::MenuBar),
        "radiogroup" => Some(Role::RadioGroup),
        "tablist" => Some(Role::TabList),
        "tree" => Some(Role::Tree),
        "treegrid" => Some(Role::TreeGrid),
        "article" => Some(Role::Article),
        "columnheader" => Some(Role::ColumnHeader),
        "definition" => Some(Role::Definition),
        "document" => Some(Role::Document),
        "group" => Some(Role::Group),
        "heading" => Some(Role::Heading),
        "img" => Some(Role::Image),
        "list" => Some(Role::List),
        "listitem" => Some(Role::ListItem),
        "math" => Some(Role::Math),
        "note" => Some(Role::Note),
        "region" => Some(Role::Region),
        "row" => Some(Role::Row),
        "rowgroup" => Some(Role::RowGroup),
        "rowheader" => Some(Role::RowHeader),
        "toolbar" => Some(Role::Toolbar),
        "application" => Some(Role::Application),
        "banner" => Some(Role::Banner),
        "complementary" => Some(Role::Complementary),
        "contentinfo" => Some(Role::ContentInfo),
        "form" => Some(Role::Form),
        "main" => Some(Role::Main),
        "navigation" => Some(Role::Navigation),
        "search" => Some(Role::Search),
        "switch" => Some(Role::Switch),
        "searchbox" => Some(Role::SearchInput),
        "separator" => Some(Role::Splitter),
        "meter" => Some(Role::Meter),
        "figure" => Some(Role::Figure),
        "table" => Some(Role::Table),
        "cell" => Some(Role::Cell),
        "paragraph" => Some(Role::Paragraph),
        "feed" => Some(Role::Feed),
        "term" => Some(Role::Term),
        "generic" | "none" | "presentation" => Some(Role::GenericContainer),
        _ => None,
    }
}

fn names_from_content(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::DefaultButton
            | Role::Cell
            | Role::CheckBox
            | Role::ColumnHeader
            | Role::DisclosureTriangle
            | Role::GridCell
            | Role::Heading
            | Role::Label
            | Role::Legend
            | Role::Link
            | Role::ListBoxOption
            | Role::MenuItem
            | Role::MenuItemCheckBox
            | Role::MenuItemRadio
            | Role::RadioButton
            | Role::Row
            | Role::RowHeader
            | Role::Switch
            | Role::Tab
            | Role::Tooltip
            | Role::TreeItem
            | Role::Caption
            | Role::FigureCaption
    )
}

fn is_form_control(tag: &str) -> bool {
    matches!(
        tag,
        "button" | "input" | "select" | "textarea" | "option" | "optgroup" | "fieldset"
    )
}

fn parse_f64(value: Option<&str>) -> Option<f64> {
    value
        .and_then(|value| value.trim_ascii().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

/// ARIA's states and properties, and the native state of form controls
/// (<https://www.w3.org/TR/wai-aria-1.2/#state_prop_def>,
/// <https://www.w3.org/TR/html-aam-1.0/>).
fn set_states_and_properties(
    builder: &mut AccessKitNode,
    element_data: &ElementData,
    tag: &str,
    role: Role,
) {
    let attr = |name| element_data.attr(name);
    let is_true = |name| attr(name) == Some("true");
    let input_type = (tag == "input").then(|| attr(local_name!("type")).unwrap_or("text"));

    // Checked and pressed: native checkboxes and radio buttons first.
    if let Some(checked) = element_data.checkbox_input_checked() {
        builder.set_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        });
    } else {
        let toggled = match role {
            Role::Button => attr(local_name!("aria-pressed")),
            _ => attr(local_name!("aria-checked")),
        };
        match toggled {
            Some("true") => builder.set_toggled(Toggled::True),
            Some("false") => builder.set_toggled(Toggled::False),
            Some("mixed") => builder.set_toggled(Toggled::Mixed),
            _ => {}
        }
    }

    match attr(local_name!("aria-current")) {
        Some("page") => builder.set_aria_current(AriaCurrent::Page),
        Some("step") => builder.set_aria_current(AriaCurrent::Step),
        Some("location") => builder.set_aria_current(AriaCurrent::Location),
        Some("date") => builder.set_aria_current(AriaCurrent::Date),
        Some("time") => builder.set_aria_current(AriaCurrent::Time),
        Some("false") | Some("") | None => {}
        Some(_) => builder.set_aria_current(AriaCurrent::True),
    }

    // Live regions: `aria-live`, or the implicit value of the live roles.
    let live = match attr(local_name!("aria-live")) {
        Some("assertive") => Some(Live::Assertive),
        Some("polite") => Some(Live::Polite),
        Some("off") => Some(Live::Off),
        _ => match role {
            Role::Alert => Some(Live::Assertive),
            Role::Status | Role::Log => Some(Live::Polite),
            _ => None,
        },
    };
    if let Some(live) = live {
        builder.set_live(live);
    }
    match attr(local_name!("aria-atomic")) {
        Some("true") => builder.set_live_atomic(),
        Some("false") => {}
        _ if matches!(role, Role::Alert | Role::Status) => builder.set_live_atomic(),
        _ => {}
    }
    if is_true(local_name!("aria-busy")) {
        builder.set_busy();
    }
    if is_true(local_name!("aria-modal")) {
        builder.set_modal();
    }

    // Values: `aria-value*`, then the native ones of range controls.
    let (now, min, max) = match (tag, input_type) {
        ("progress", _) => {
            let max = parse_f64(attr(local_name!("max"))).filter(|max| *max > 0.0);
            (
                parse_f64(attr(local_name!("value"))),
                Some(0.0),
                Some(max.unwrap_or(1.0)),
            )
        }
        ("meter", _) | (_, Some("range" | "number")) => (
            parse_f64(attr(local_name!("value"))).or_else(|| {
                element_data
                    .text_input_data()
                    .and_then(|input| input.editor.raw_text().trim_ascii().parse().ok())
            }),
            parse_f64(attr(local_name!("min"))),
            parse_f64(attr(local_name!("max"))),
        ),
        _ => (None, None, None),
    };
    if let Some(now) = parse_f64(attr(local_name!("aria-valuenow"))).or(now) {
        builder.set_numeric_value(now);
    }
    if let Some(min) = parse_f64(attr(local_name!("aria-valuemin"))).or(min) {
        builder.set_min_numeric_value(min);
    }
    if let Some(max) = parse_f64(attr(local_name!("aria-valuemax"))).or(max) {
        builder.set_max_numeric_value(max);
    }
    if let Some(text) = attr(local_name!("aria-valuetext")) {
        builder.set_value(text);
    } else if let Some(input) = element_data.text_input_data() {
        // A password's characters are not read out.
        if input_type == Some("password") {
            builder.set_value("\u{2022}".repeat(input.editor.raw_text().chars().count()));
        } else {
            builder.set_value(input.editor.raw_text());
        }
    }
    if let Some(placeholder) = attr(local_name!("aria-placeholder")).or_else(|| {
        matches!(tag, "input" | "textarea").then(|| attr(local_name!("placeholder")))?
    }) {
        builder.set_placeholder(placeholder);
    }

    match attr(local_name!("aria-invalid")) {
        Some("grammar") => builder.set_invalid(Invalid::Grammar),
        Some("spelling") => builder.set_invalid(Invalid::Spelling),
        Some("false") | Some("") | None => {}
        Some(_) => builder.set_invalid(Invalid::True),
    }
    if is_true(local_name!("aria-required"))
        || (matches!(tag, "input" | "select" | "textarea")
            && attr(local_name!("required")).is_some())
    {
        builder.set_required();
    }
    if is_true(local_name!("aria-readonly"))
        || (matches!(tag, "input" | "textarea") && attr(local_name!("readonly")).is_some())
    {
        builder.set_read_only();
    }
    if is_true(local_name!("aria-multiselectable"))
        || (tag == "select" && attr(local_name!("multiple")).is_some())
    {
        builder.set_multiselectable();
    }

    match attr(local_name!("aria-haspopup")) {
        Some("menu") | Some("true") => builder.set_has_popup(HasPopup::Menu),
        Some("listbox") => builder.set_has_popup(HasPopup::Listbox),
        Some("tree") => builder.set_has_popup(HasPopup::Tree),
        Some("grid") => builder.set_has_popup(HasPopup::Grid),
        Some("dialog") => builder.set_has_popup(HasPopup::Dialog),
        _ => {}
    }
    match attr(local_name!("aria-orientation")) {
        Some("horizontal") => builder.set_orientation(Orientation::Horizontal),
        Some("vertical") => builder.set_orientation(Orientation::Vertical),
        _ => {}
    }

    // `aria-posinset` and `aria-setsize` are 1-based, like AccessKit's.
    let parse_usize =
        |name| attr(name).and_then(|value: &str| value.trim_ascii().parse::<usize>().ok());
    if let Some(position) = parse_usize(local_name!("aria-posinset")).filter(|p| *p > 0) {
        builder.set_position_in_set(position);
    }
    if let Some(size) = parse_usize(local_name!("aria-setsize")) {
        builder.set_size_of_set(size);
    }

    if role == Role::Link
        && let Some(href) = attr(local_name!("href"))
    {
        builder.set_url(href);
    }
}

/// What an assistive technology can do with the element; the shell routes
/// its requests to DOM events (`UiEvent::Accessibility`).
fn add_actions(
    builder: &mut AccessKitNode,
    node: &BlitzDomNode,
    element_data: &ElementData,
    role: Role,
) {
    builder.add_action(Action::ScrollIntoView);
    if builder.is_disabled() {
        return;
    }
    let focusable = node.is_focussable();
    if focusable {
        builder.add_action(Action::Focus);
    }
    if focusable
        || matches!(
            role,
            Role::Button
                | Role::DefaultButton
                | Role::Link
                | Role::CheckBox
                | Role::RadioButton
                | Role::Switch
                | Role::MenuItem
                | Role::MenuItemCheckBox
                | Role::MenuItemRadio
                | Role::Tab
                | Role::ListBoxOption
                | Role::TreeItem
                | Role::DisclosureTriangle
                | Role::ComboBox
        )
    {
        builder.add_action(Action::Click);
    }
    match element_data.attr(local_name!("aria-expanded")) {
        Some("true") => builder.add_action(Action::Collapse),
        Some("false") => builder.add_action(Action::Expand),
        _ => {}
    }
    if matches!(role, Role::Slider | Role::SpinButton) {
        builder.add_action(Action::Increment);
        builder.add_action(Action::Decrement);
        builder.add_action(Action::SetValue);
    }
}

fn default_level_for_tag(name: &str) -> Option<usize> {
    // Accesskit levels are 0-based, while ARIA levels are 1-based
    // See https://docs.rs/accesskit/latest/accesskit/struct.Node.html#method.level
    match name {
        "h1" => Some(0),
        "h2" => Some(1),
        "h3" => Some(2),
        "h4" => Some(3),
        "h5" => Some(4),
        "h6" => Some(5),
        _ => None,
    }
}

fn default_level_for_role(role: Role) -> Option<usize> {
    match role {
        // ARIA specifies an implicit level of 2 for the heading role. Accesskit levels are 0-based,
        // so this turns into level 1
        Role::Heading => Some(1),
        _ => None,
    }
}

fn role_from_element_data(element_data: &ElementData) -> Option<Role> {
    // <https://www.w3.org/TR/html-aam-1.0/>
    match &*element_data.name.local {
        // Document structure
        "article" => Some(Role::Article),
        "aside" => Some(Role::Complementary),
        "footer" => Some(Role::Footer),
        "header" => Some(Role::Header),
        "main" => Some(Role::Main),
        "nav" => Some(Role::Navigation),
        "search" => Some(Role::Search),
        "section" => Some(Role::Section),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(Role::Heading),
        "p" => Some(Role::Paragraph),
        "blockquote" => Some(Role::Blockquote),
        "figure" => Some(Role::Figure),
        "figcaption" | "caption" => Some(Role::Caption),
        "hr" => Some(Role::Splitter),

        // Grouping
        "ul" | "ol" | "menu" => Some(Role::List),
        "li" => Some(Role::ListItem),
        "dl" => Some(Role::DescriptionList),
        "dt" => Some(Role::Term),
        "dd" => Some(Role::Definition),
        "dialog" => Some(Role::Dialog),
        "fieldset" => Some(Role::Group),
        "form" => Some(Role::Form),
        "div" => Some(Role::GenericContainer),

        // Tables
        "table" => Some(Role::Table),
        "thead" | "tbody" | "tfoot" => Some(Role::RowGroup),
        "tr" => Some(Role::Row),
        "td" => Some(Role::Cell),
        "th" => match element_data.attr(local_name!("scope")) {
            Some("row") | Some("rowgroup") => Some(Role::RowHeader),
            _ => Some(Role::ColumnHeader),
        },

        // Interactive
        // An <a> is only a link when it has an href.
        "a" => match element_data.attr(local_name!("href")) {
            Some(_) => Some(Role::Link),
            None => Some(Role::GenericContainer),
        },
        "button" => Some(Role::Button),
        "label" => Some(Role::Label),
        "legend" => Some(Role::Label),
        "select" => match element_data.attr(local_name!("multiple")) {
            Some(_) => Some(Role::ListBox),
            None => Some(Role::ComboBox),
        },
        "option" => Some(Role::ListBoxOption),
        "textarea" => Some(Role::MultilineTextInput),
        "progress" => Some(Role::ProgressIndicator),
        "meter" => Some(Role::Meter),
        "output" => Some(Role::Status),
        "summary" => Some(Role::DisclosureTriangle),

        // Inline semantics
        "code" => Some(Role::Code),
        "em" => Some(Role::Emphasis),
        "strong" => Some(Role::Strong),
        "mark" => Some(Role::Mark),
        "time" => Some(Role::Time),
        "img" => Some(Role::Image),
        "iframe" => Some(Role::Iframe),

        "input" => {
            let ty = element_data.attr(local_name!("type")).unwrap_or("text");
            match ty {
                "button" | "submit" | "reset" => Some(Role::Button),
                "checkbox" => Some(Role::CheckBox),
                "color" => Some(Role::ColorWell),
                "date" => Some(Role::DateInput),
                "datetime-local" => Some(Role::DateTimeInput),
                "email" => Some(Role::EmailInput),
                "number" => Some(Role::NumberInput),
                "password" => Some(Role::PasswordInput),
                "radio" => Some(Role::RadioButton),
                "range" => Some(Role::Slider),
                "search" => Some(Role::SearchInput),
                "tel" => Some(Role::PhoneNumberInput),
                "time" => Some(Role::TimeInput),
                _ => Some(Role::TextInput),
            }
        }
        _ => None,
    }
}
