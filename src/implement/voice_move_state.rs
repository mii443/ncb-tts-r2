use serenity::model::{prelude::ChannelId, voice::VoiceState};

pub trait VoiceMoveStateTrait {
    fn move_state(&self, old: &Option<VoiceState>, target_channel: ChannelId) -> VoiceMoveState;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceMoveState {
    JOIN,
    LEAVE,
    NONE,
}

impl VoiceMoveStateTrait for VoiceState {
    fn move_state(&self, old: &Option<VoiceState>, target_channel: ChannelId) -> VoiceMoveState {
        let new = self;

        if let None = old.clone() {
            return if target_channel == new.channel_id.unwrap() {
                VoiceMoveState::JOIN
            } else {
                VoiceMoveState::NONE
            };
        }

        let old = (*old).clone().unwrap();

        match (old.channel_id, new.channel_id) {
            (Some(old_channel_id), Some(new_channel_id)) => {
                if old_channel_id == new_channel_id {
                    VoiceMoveState::NONE
                } else if old_channel_id != new_channel_id {
                    if target_channel == new_channel_id {
                        VoiceMoveState::JOIN
                    } else {
                        VoiceMoveState::NONE
                    }
                } else {
                    VoiceMoveState::NONE
                }
            }
            (Some(old_channel_id), None) => {
                if old_channel_id == target_channel {
                    VoiceMoveState::LEAVE
                } else {
                    VoiceMoveState::NONE
                }
            }
            _ => VoiceMoveState::NONE,
        }
    }
}
