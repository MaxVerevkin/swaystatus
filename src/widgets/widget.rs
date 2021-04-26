use super::{Spacing, State};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_block::I3BarBlock;

#[derive(Clone, Debug)]
pub struct Widget {
    content: Option<String>,
    icon: Option<String>,
    spacing: Spacing,
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
            content: None,
            icon: None,
            spacing: Spacing::Normal,
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

    pub fn with_text(mut self, content: String) -> Self {
        self.set_text(content);
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

    pub fn set_text(&mut self, content: String) {
        if content.is_empty() {
            self.spacing = Spacing::Hidden;
        } else {
            self.spacing = Spacing::Normal;
        }
        self.content = Some(content);
    }

    pub fn set_state(&mut self, state: State) {
        let (key_bg, key_fg) = state.theme_keys(&self.shared_config.theme);

        self.inner.background = key_bg.clone();
        self.inner.color = key_fg.clone();
    }

    pub fn set_spacing(&mut self, spacing: Spacing) {
        self.spacing = spacing;
    }

    pub fn get_data(&self) -> I3BarBlock {
        let mut data = self.inner.clone();

        data.full_text = format!(
            "{}{}{}",
            self.icon.clone().unwrap_or_else(|| {
                match self.spacing {
                    Spacing::Normal => " ",
                    Spacing::Inline => "",
                    Spacing::Hidden => "",
                }
                .to_string()
            }),
            self.content.clone().unwrap_or_default(),
            match self.spacing {
                Spacing::Normal => " ",
                Spacing::Inline => " ",
                Spacing::Hidden => "",
            }
            .to_string()
        );

        data
    }
}
