pub mod i3bar_block;
pub mod i3bar_event;

use crate::config::SharedConfig;
use crate::errors::*;
use crate::util::add_colors;

use i3bar_block::I3BarBlock;

pub fn init(never_pause: bool) {
    if never_pause {
        println!("{{\"version\": 1, \"click_events\": true, \"stop_signal\": 0}}\n[");
    } else {
        println!("{{\"version\": 1, \"click_events\": true}}\n[");
    }
}

pub fn print_blocks(blocks: &[Vec<I3BarBlock>], config: &SharedConfig) -> Result<()> {
    let mut last_bg: Option<String> = None;

    let mut rendered_blocks = vec![];

    /* To always start with the same alternating tint on the right side of the
     * bar it is easiest to calculate the number of visible blocks here and
     * flip the starting tint if an even number of blocks is visible. This way,
     * the last block should always be untinted.
     */
    let mut visible_count = 0;
    for x in blocks.iter() {
        visible_count += x.len();
    }

    let mut alternator = visible_count % 2 == 0;

    for widgets in blocks.iter() {
        if widgets.is_empty() {
            continue;
        }

        let mut rendered_widgets: Vec<I3BarBlock> = widgets
            .iter()
            .map(|data| {
                let mut data = data.clone();
                if alternator {
                    // Apply tint for all widgets of every second block
                    data.background = add_colors(
                        data.background.as_deref(),
                        config.theme.alternating_tint_bg.as_deref(),
                    )
                    .unwrap();
                    data.color = add_colors(
                        data.color.as_deref(),
                        config.theme.alternating_tint_bg.as_deref(),
                    )
                    .unwrap();
                }
                data
            })
            .collect();

        alternator = !alternator;

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
        let sep_fg = if config.theme.separator_fg == Some("auto".to_string()) {
            rendered_widgets.first().unwrap().background.clone()
        } else {
            config.theme.separator_fg.clone()
        };

        // The separator's BG is the last block's last widget's BG
        let sep_bg = if config.theme.separator_bg == Some("auto".to_string()) {
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