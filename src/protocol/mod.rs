pub mod i3bar_block;
pub mod i3bar_event;

use crate::config::SharedConfig;
use crate::errors::*;
use crate::themes::Color;

use i3bar_block::I3BarBlock;

pub fn init(never_pause: bool) {
    if never_pause {
        println!("{{\"version\": 1, \"click_events\": true, \"stop_signal\": 0}}\n[");
    } else {
        println!("{{\"version\": 1, \"click_events\": true}}\n[");
    }
}

pub fn print_blocks(blocks: &[Vec<I3BarBlock>], config: &SharedConfig) -> Result<()> {
    let mut last_bg = Color::None;

    let mut rendered_blocks = vec![];

    // The right most block should never be alternated
    let mut alt = true;
    for x in blocks.iter() {
        if !x.is_empty() {
            alt = !alt;
        }
    }

    for widgets in blocks.iter() {
        if widgets.is_empty() {
            continue;
        }

        let mut rendered_widgets: Vec<I3BarBlock> = widgets
            .iter()
            .map(|data| {
                let mut data = data.clone();
                if alt {
                    // Apply tint for all widgets of every second block
                    // TODO: Allow for other non-additive tints
                    data.background = data.background + config.theme.alternating_tint_bg;
                    data.color = data.color + config.theme.alternating_tint_fg;
                }
                data
            })
            .collect();

        alt = !alt;

        if config.theme.separator.is_none() {
            // Re-add native separator on last widget for native theme
            rendered_widgets.last_mut().unwrap().separator = None;
            rendered_widgets.last_mut().unwrap().separator_block_width = None;
        }

        // Serialize and concatenate widgets
        let block_str = rendered_widgets
            .iter()
            .map(|w| w.render())
            .collect::<Vec<String>>()
            .join(",");

        if config.theme.separator.is_none() {
            // Skip separator block for native theme
            rendered_blocks.push(block_str.to_string());
            continue;
        }

        // The first widget's BG is used to get the FG color for the current separator
        let sep_fg = if config.theme.separator_fg == Color::Auto {
            rendered_widgets.first().unwrap().background.clone()
        } else {
            config.theme.separator_fg.clone()
        };

        // The separator's BG is the last block's last widget's BG
        let sep_bg = if config.theme.separator_bg == Color::Auto {
            last_bg
        } else {
            config.theme.separator_bg.clone()
        };

        if let Some(ref separator) = config.theme.separator {
            let separator = I3BarBlock {
                full_text: separator.clone(),
                background: sep_bg,
                color: sep_fg,
                ..Default::default()
            };
            rendered_blocks.push(format!("{},{}", separator.render(), block_str));
        } else {
            rendered_blocks.push(block_str);
        }

        // The last widget's BG is used to get the BG color for the next separator
        last_bg = rendered_widgets.last().unwrap().background.clone();
    }

    println!("[{}],", rendered_blocks.join(","));

    Ok(())
}
