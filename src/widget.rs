use crate::config::SharedConfig;
use crate::protocol::i3bar_block::I3BarBlock;
use crate::themes::{Color, Theme};
use serde_derive::Deserialize;
use smartstring::alias::String;

#[derive(Debug, Copy, Clone, Deserialize)]
pub enum WidgetSpacing {
    /// Add a leading and trailing space around the widget contents
    Normal,
    /// Hide the leading space when the widget is inline
    Inline,
    /// Hide both leading and trailing spaces when widget is hidden
    Hidden,
}

#[derive(Debug, Copy, Clone, Deserialize)]
pub enum WidgetState {
    Idle,
    Info,
    Good,
    Warning,
    Critical,
}

impl WidgetState {
    pub fn theme_keys(self, theme: &Theme) -> (Color, Color) {
        use self::WidgetState::*;
        match self {
            Idle => (theme.idle_bg, theme.idle_fg),
            Info => (theme.info_bg, theme.info_fg),
            Good => (theme.good_bg, theme.good_fg),
            Warning => (theme.warning_bg, theme.warning_fg),
            Critical => (theme.critical_bg, theme.critical_fg),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Widget {
    pub instance: Option<usize>,
    full_text: String,
    short_text: Option<String>,
    pub icon: String,
    full_spacing: WidgetSpacing,
    short_spacing: WidgetSpacing,
    pub shared_config: SharedConfig,
    inner: I3BarBlock,
}

impl Widget {
    pub fn new(id: usize, shared_config: SharedConfig) -> Self {
        let (key_bg, key_fg) = WidgetState::Idle.theme_keys(&shared_config.theme); // Initial colors
        let inner = I3BarBlock {
            name: Some(id.to_string()),
            color: key_fg,
            background: key_bg,
            ..I3BarBlock::default()
        };

        Widget {
            instance: None,
            full_text: String::new(),
            short_text: None,
            icon: String::new(),
            full_spacing: WidgetSpacing::Hidden,
            short_spacing: WidgetSpacing::Hidden,
            shared_config,
            inner,
        }
    }

    /*
     * Consturctors
     */

    pub fn with_instance(mut self, instance: usize) -> Self {
        self.instance = Some(instance);
        self.inner.instance = Some(instance.to_string());
        self
    }

    pub fn with_icon_str(mut self, icon: String) -> Self {
        self.icon = icon;
        self
    }

    pub fn with_full_text(mut self, content: String) -> Self {
        self.set_full_text(content);
        self
    }

    pub fn with_state(mut self, state: WidgetState) -> Self {
        self.set_state(state);
        self
    }

    // pub fn with_spacing(mut self, spacing: WidgetSpacing) -> Self {
    //     self.set_spacing(spacing);
    //     self
    // }

    /*
     * Setters
     */

    pub fn set_text(&mut self, content: (String, Option<String>)) {
        if content.0.is_empty() {
            self.full_spacing = WidgetSpacing::Hidden;
        } else {
            self.full_spacing = WidgetSpacing::Normal;
        }
        if content.1.as_ref().map(String::is_empty).unwrap_or(true) {
            self.short_spacing = WidgetSpacing::Hidden;
        } else {
            self.short_spacing = WidgetSpacing::Normal;
        }
        self.full_text = content.0;
        self.short_text = content.1;
    }
    pub fn set_full_text(&mut self, content: String) {
        if content.is_empty() {
            self.full_spacing = WidgetSpacing::Hidden;
        } else {
            self.full_spacing = WidgetSpacing::Normal;
        }
        self.full_text = content;
    }

    pub fn set_state(&mut self, state: WidgetState) {
        let (key_bg, key_fg) = state.theme_keys(&self.shared_config.theme);

        self.inner.background = key_bg;
        self.inner.color = key_fg;
    }

    #[allow(dead_code)]
    pub fn set_spacing(&mut self, spacing: WidgetSpacing) {
        self.full_spacing = spacing;
        self.short_spacing = spacing;
    }

    /// Constuct `I3BarBlock` from this widget
    pub fn get_data(&self) -> I3BarBlock {
        let mut data = self.inner.clone();

        data.full_text = format!(
            "{}{}{}",
            match (self.icon.as_str(), self.full_spacing) {
                ("", WidgetSpacing::Normal) => " ",
                ("", WidgetSpacing::Inline) => "",
                ("", WidgetSpacing::Hidden) => "",
                (icon, _) => icon,
            },
            self.full_text,
            match self.full_spacing {
                WidgetSpacing::Normal => " ",
                WidgetSpacing::Inline => " ",
                WidgetSpacing::Hidden => "",
            }
        );

        data.short_text = self.short_text.as_ref().map(|short_text| {
            format!(
                "{}{}{}",
                match (self.icon.as_str(), self.short_spacing) {
                    ("", WidgetSpacing::Normal) => " ",
                    ("", WidgetSpacing::Inline) => "",
                    ("", WidgetSpacing::Hidden) => "",
                    (icon, _) => icon,
                },
                short_text,
                match self.short_spacing {
                    WidgetSpacing::Normal => " ",
                    WidgetSpacing::Inline => " ",
                    WidgetSpacing::Hidden => "",
                }
            )
        });

        data
    }
}
