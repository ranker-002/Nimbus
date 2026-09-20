use adw::prelude::*;

/// One entry in the sidebar and its matching stack page.
pub struct NavEntry<'a> {
    pub tag: &'a str,
    pub title: &'a str,
    pub icon: &'a str,
    pub widget: &'a gtk::Widget,
}

/// The assembled sidebar/content split, plus a way to switch pages from code
/// (the first-run page needs to send the user to the hotspot form).
pub struct Navigation {
    split_view: adw::NavigationSplitView,
    stack: gtk::Stack,
    listbox: gtk::ListBox,
}

impl Navigation {
    pub fn widget(&self) -> &adw::NavigationSplitView {
        &self.split_view
    }

    /// Brings the page with `tag` to the front and highlights it in the
    /// sidebar.
    pub fn show(&self, tag: &str) {
        self.stack.set_visible_child_name(tag);
        for index in 0..self.listbox.observe_children().n_items() {
            let Some(row) = self
                .listbox
                .observe_children()
                .item(index)
                .and_then(|o| o.downcast::<gtk::ListBoxRow>().ok())
            else {
                continue;
            };
            if row.widget_name() == tag {
                self.listbox.select_row(Some(&row));
                break;
            }
        }
    }
}

/// Builds the sidebar/content split for the given pages.
///
/// This is deliberately free of any backend knowledge: pages are constructed
/// and wired up by the window, which owns them.
pub fn build(entries: &[NavEntry<'_>]) -> Navigation {
    let split_view = adw::NavigationSplitView::builder()
        .min_sidebar_width(200.0)
        .max_sidebar_width(280.0)
        .sidebar_width_fraction(0.3)
        .build();

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);

    let listbox = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .build();

    for entry in entries {
        stack.add_titled(entry.widget, Some(entry.tag), entry.title);
        listbox.append(&create_nav_row(entry.title, entry.icon, entry.tag));
    }

    let stack_clone = stack.clone();
    listbox.connect_row_activated(move |_, row| {
        stack_clone.set_visible_child_name(row.widget_name().as_str());
    });

    if let Some(first) = listbox.row_at_index(0) {
        listbox.select_row(Some(&first));
    }

    let sidebar = adw::NavigationPage::builder()
        .title("Nimbus")
        .child(&listbox)
        .build();
    split_view.set_sidebar(Some(&sidebar));

    let content = adw::NavigationPage::builder()
        .title("Nimbus Hotspot")
        .child(&stack)
        .build();
    split_view.set_content(Some(&content));

    Navigation {
        split_view,
        stack,
        listbox,
    }
}

fn create_nav_row(label: &str, icon: &str, tag: &str) -> gtk::ListBoxRow {
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hbox.set_margin_top(8);
    hbox.set_margin_bottom(8);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);

    let image = gtk::Image::from_icon_name(icon);
    image.set_valign(gtk::Align::Center);
    hbox.append(&image);
    hbox.append(&gtk::Label::new(Some(label)));

    let row = gtk::ListBoxRow::builder()
        .child(&hbox)
        .activatable(true)
        .build();
    row.set_widget_name(tag);
    row
}
