use super::{Spacing, State};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_block::I3BarBlock;

#[derive(Clone, Debug)]
pub struct Widget {
    full_text: Option<String>,
    short_text: Option<String>,
    icon: Option<String>,
    full_spacing: Spacing,
    short_spacing: Spacing,
    shared_config: SharedConfig,
    inner: I3BarBlock,
}

impl Widget {
    pub fn new(id: usize, shared_config: SharedConfig) -> Self {
        let (key_bg, key_fg) = State::Idle.theme_keys(&shared_config.theme); // Initial colors
        let inner = I3BarBlock {
            name: Some(id.to_string()),
            color: key_fg.clone(),
            background: key_bg.clone(),
            ..I3BarBlock::default()
        };

        Widget {
            full_text: None,
            short_text: None,
            icon: None,
            full_spacing: Spacing::Hidden,
            short_spacing: Spacing::Hidden,
            shared_config,
            inner,
        }
    }

    pub fn with_instance(mut self, instance: usize) -> Self {
        self.inner.instance = Some(instance.to_string());
        self
    }

    pub fn with_icon(mut self, name: &str) -> Result<Self> {
        self.set_icon(name)?;
        Ok(self)
    }

    pub fn with_text(mut self, content: (String, Option<String>)) -> Self {
        self.set_text(content);
        self
    }
    pub fn with_full_text(mut self, content: String) -> Self {
        self.set_full_text(content);
        self
    }

    pub fn with_state(mut self, state: State) -> Self {
        self.set_state(state);
        self
    }

    pub fn with_spacing(mut self, spacing: Spacing) -> Self {
        self.set_spacing(spacing);
        self
    }

    pub fn set_icon(&mut self, name: &str) -> Result<()> {
        self.icon = Some(self.shared_config.get_icon(name)?);
        Ok(())
    }

    pub fn set_text(&mut self, content: (String, Option<String>)) {
        if content.0.is_empty() {
            self.full_spacing = Spacing::Hidden;
        } else {
            self.full_spacing = Spacing::Normal;
        }
        if content.1.as_ref().map(String::is_empty).unwrap_or(true) {
            self.short_spacing = Spacing::Hidden;
        } else {
            self.short_spacing = Spacing::Normal;
        }
        self.full_text = Some(content.0);
        self.short_text = content.1;
    }
    pub fn set_full_text(&mut self, content: String) {
        if content.is_empty() {
            self.full_spacing = Spacing::Hidden;
        } else {
            self.full_spacing = Spacing::Normal;
        }
        self.full_text = Some(content);
    }

    pub fn set_state(&mut self, state: State) {
        let (key_bg, key_fg) = state.theme_keys(&self.shared_config.theme);

        self.inner.background = key_bg.clone();
        self.inner.color = key_fg.clone();
    }

    pub fn set_spacing(&mut self, spacing: Spacing) {
        self.full_spacing = spacing;
        self.short_spacing = spacing;
    }

    pub fn get_data(&self) -> I3BarBlock {
        let mut data = self.inner.clone();

        data.full_text = format!(
            "{}{}{}",
            self.icon.clone().unwrap_or_else(|| {
                match self.full_spacing {
                    Spacing::Normal => " ",
                    Spacing::Inline => "",
                    Spacing::Hidden => "",
                }
                .to_string()
            }),
            self.full_text.clone().unwrap_or_default(),
            match self.full_spacing {
                Spacing::Normal => " ",
                Spacing::Inline => " ",
                Spacing::Hidden => "",
            }
            .to_string()
        );

        data.short_text = self.short_text.as_ref().map(|short_text| {
            format!(
                "{}{}{}",
                self.icon.clone().unwrap_or_else(|| {
                    match self.short_spacing {
                        Spacing::Normal => " ",
                        Spacing::Inline => "",
                        Spacing::Hidden => "",
                    }
                    .to_string()
                }),
                short_text,
                match self.short_spacing {
                    Spacing::Normal => " ",
                    Spacing::Inline => " ",
                    Spacing::Hidden => "",
                }
                .to_string()
            )
        });

        data
    }
}
