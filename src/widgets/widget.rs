use super::{I3BarWidget, Spacing, State};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_block::I3BarBlock;

#[derive(Clone, Debug)]
pub struct Widget {
    id: usize,
    instance: usize,
    content: Option<String>,
    icon: Option<String>,
    state: State,
    spacing: Spacing,
    shared_config: SharedConfig,
    inner: I3BarBlock,
}

impl Widget {
    pub fn new(id: usize, instance: usize, shared_config: SharedConfig) -> Self {
        let (key_bg, key_fg) = State::Idle.theme_keys(&shared_config.theme); // Initial colors
        let inner = I3BarBlock {
            name: Some(id.to_string()),
            instance: Some(instance.to_string()),
            color: key_fg.clone(),
            background: key_bg.clone(),
            ..I3BarBlock::default()
        };

        Widget {
            id,
            instance,
            content: None,
            icon: None,
            state: State::Idle,
            spacing: Spacing::Normal,
            shared_config,
            inner,
        }
    }

    pub fn with_icon(mut self, name: &str) -> Result<Self> {
        self.icon = Some(self.shared_config.get_icon(name)?);
        self.update();
        Ok(self)
    }

    pub fn with_text(mut self, content: &str) -> Self {
        self.content = Some(String::from(content));
        self.update();
        self
    }

    pub fn with_state(mut self, state: State) -> Self {
        self.state = state;
        self.update();
        self
    }

    #[allow(dead_code)]
    pub fn with_spacing(mut self, spacing: Spacing) -> Self {
        self.spacing = spacing;
        self.update();
        self
    }

    #[allow(dead_code)]
    pub fn set_icon(&mut self, name: &str) -> Result<()> {
        self.icon = Some(self.shared_config.get_icon(name)?);
        self.update();
        Ok(())
    }

    pub fn set_text(&mut self, content: String) {
        if content.is_empty() {
            self.spacing = Spacing::Hidden;
        } else {
            self.spacing = Spacing::Normal;
        }
        self.content = Some(content);
        self.update();
    }

    pub fn set_state(&mut self, state: State) {
        self.state = state;
        self.update();
    }

    #[allow(dead_code)]
    pub fn set_spacing(&mut self, spacing: Spacing) {
        self.spacing = spacing;
        self.update();
    }

    fn update(&mut self) {
        let (key_bg, key_fg) = self.state.theme_keys(&self.shared_config.theme);

        // When rendered inline, remove the leading space
        self.inner.full_text = format!(
            "{}{}{}",
            self.icon.clone().unwrap_or_else(|| {
                match self.spacing {
                    Spacing::Normal => String::from(" "),
                    _ => String::from(""),
                }
            }),
            self.content.clone().unwrap_or_default(),
            match self.spacing {
                Spacing::Hidden => String::from(""),
                _ => String::from(" "),
            }
        );
        self.inner.background = key_bg.clone();
        self.inner.color = key_fg.clone();
    }
}

impl I3BarWidget for Widget {
    fn get_data(&self) -> I3BarBlock {
        self.inner.clone()
    }
}
