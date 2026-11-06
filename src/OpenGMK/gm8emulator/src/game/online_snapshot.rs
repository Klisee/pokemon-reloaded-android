use crate::{
    game::{gm_save::GMSave, Game},
    gml::rand::Random,
};

pub(crate) struct OnlineSnapshot {
    save: GMSave,
    rand: Random,
    view_current: usize,
}

impl OnlineSnapshot {
    pub(crate) fn capture(game: &Game) -> Self {
        Self {
            save: GMSave::from_game(game),
            rand: game.rand.clone(),
            view_current: game.view_current,
        }
    }

    pub(crate) fn restore(self, game: &mut Game) -> Result<(), String> {
        let Self { save, rand, view_current } = self;
        save.into_game(game)?;
        game.rand = rand;
        game.view_current = view_current;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_and_restore_have_expected_signatures() {
        let _capture: fn(&Game) -> OnlineSnapshot = OnlineSnapshot::capture;
        let _restore: fn(OnlineSnapshot, &mut Game) -> Result<(), String> =
            OnlineSnapshot::restore;
    }

    #[test]
    fn random_state_round_trips() {
        let mut rand = Random::with_seed(0x1234_5678);
        let saved = rand.clone();
        rand.next(100.0);
        assert!(rand != saved);
        rand = saved.clone();
        assert!(rand == saved);
    }
}
