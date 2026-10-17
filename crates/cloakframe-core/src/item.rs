use std::fmt;

use serde::{Deserialize, Serialize};

use crate::coverage::{CoverageEvidence, has_unresolved};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewMode {
    Required,
    /// Items without unresolved problems are published as automatically processed and unreviewed.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishBasis {
    AutomaticUnreviewed,
    Reviewed,
    /// Chosen explicitly per item despite unresolved problems.
    ExportedWithWarnings,
}

impl PublishBasis {
    pub fn counts_as_normal_completion(self) -> bool {
        self != Self::ExportedWithWarnings
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    Queued,
    Analyzed,
    NeedsReview,
    ReadyForRender(PublishBasis),
    Rendering(PublishBasis),
    Verified(PublishBasis),
    Published(PublishBasis),
    Failed,
    Cancelled,
}

impl ItemState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Published(_) | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    NotAllowed {
        from: ItemState,
        action: &'static str,
    },
    UnresolvedProblems,
    PermitMismatch,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAllowed { from, action } => write!(f, "cannot {action} from {from:?}"),
            Self::UnresolvedProblems => f.write_str("unresolved detection problems remain"),
            Self::PermitMismatch => {
                f.write_str("publish permit was issued for another item or basis")
            }
        }
    }
}

impl std::error::Error for TransitionError {}

/// Processing state of one input. Normal path: Queued → Analyzed → NeedsReview or ReadyForRender
/// → Rendering → Verified → Published. Failure and cancellation end any non-terminal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    id: ItemId,
    state: ItemState,
}

impl Item {
    pub fn new(id: ItemId) -> Self {
        Self {
            id,
            state: ItemState::Queued,
        }
    }

    /// An item whose analysis was stored in a review session and is being resumed. It still has to
    /// go through review approval or an explicit export with warnings before it can render.
    pub fn awaiting_review(id: ItemId) -> Self {
        Self {
            id,
            state: ItemState::NeedsReview,
        }
    }

    pub fn id(&self) -> ItemId {
        self.id
    }

    pub fn state(&self) -> ItemState {
        self.state
    }

    fn not_allowed(&self, action: &'static str) -> TransitionError {
        TransitionError::NotAllowed {
            from: self.state,
            action,
        }
    }

    pub fn finish_analysis(&mut self) -> Result<(), TransitionError> {
        match self.state {
            ItemState::Queued => {
                self.state = ItemState::Analyzed;
                Ok(())
            }
            _ => Err(self.not_allowed("finish analysis")),
        }
    }

    pub fn route(
        &mut self,
        mode: ReviewMode,
        evidence: &[CoverageEvidence],
    ) -> Result<(), TransitionError> {
        if self.state != ItemState::Analyzed {
            return Err(self.not_allowed("route"));
        }
        self.state = if mode == ReviewMode::Off && !has_unresolved(evidence) {
            ItemState::ReadyForRender(PublishBasis::AutomaticUnreviewed)
        } else {
            ItemState::NeedsReview
        };
        Ok(())
    }

    pub fn approve_review(&mut self, evidence: &[CoverageEvidence]) -> Result<(), TransitionError> {
        if self.state != ItemState::NeedsReview {
            return Err(self.not_allowed("approve review"));
        }
        if has_unresolved(evidence) {
            return Err(TransitionError::UnresolvedProblems);
        }
        self.state = ItemState::ReadyForRender(PublishBasis::Reviewed);
        Ok(())
    }

    pub fn export_with_warnings(&mut self) -> Result<(), TransitionError> {
        if self.state != ItemState::NeedsReview {
            return Err(self.not_allowed("export with warnings"));
        }
        self.state = ItemState::ReadyForRender(PublishBasis::ExportedWithWarnings);
        Ok(())
    }

    pub fn start_render(&mut self) -> Result<(), TransitionError> {
        match self.state {
            ItemState::ReadyForRender(basis) => {
                self.state = ItemState::Rendering(basis);
                Ok(())
            }
            _ => Err(self.not_allowed("start render")),
        }
    }

    pub fn mark_verified(&mut self) -> Result<(), TransitionError> {
        match self.state {
            ItemState::Rendering(basis) => {
                self.state = ItemState::Verified(basis);
                Ok(())
            }
            _ => Err(self.not_allowed("mark verified")),
        }
    }

    pub fn publish(&mut self, permit: PublishPermit) -> Result<(), TransitionError> {
        match self.state {
            ItemState::Verified(basis) if permit.item == self.id && permit.basis == basis => {
                self.state = ItemState::Published(basis);
                Ok(())
            }
            ItemState::Verified(_) => Err(TransitionError::PermitMismatch),
            _ => Err(self.not_allowed("publish")),
        }
    }

    pub fn fail(&mut self) -> Result<(), TransitionError> {
        self.end(ItemState::Failed, "fail")
    }

    pub fn cancel(&mut self) -> Result<(), TransitionError> {
        self.end(ItemState::Cancelled, "cancel")
    }

    fn end(&mut self, to: ItemState, action: &'static str) -> Result<(), TransitionError> {
        if self.state.is_terminal() {
            return Err(self.not_allowed(action));
        }
        self.state = to;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermitRefusal {
    NotVerified(ItemState),
    UnresolvedProblems,
}

impl fmt::Display for PermitRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotVerified(state) => write!(f, "item is {state:?}, not verified"),
            Self::UnresolvedProblems => f.write_str("unresolved detection problems remain"),
        }
    }
}

impl std::error::Error for PermitRefusal {}

/// The only way to move an item to `Published`. It can be issued only for a verified item, and
/// only without unresolved problems unless the item was explicitly exported with warnings (P-1).
#[derive(Debug, PartialEq, Eq)]
pub struct PublishPermit {
    item: ItemId,
    basis: PublishBasis,
}

impl PublishPermit {
    pub fn issue(item: &Item, evidence: &[CoverageEvidence]) -> Result<Self, PermitRefusal> {
        let ItemState::Verified(basis) = item.state else {
            return Err(PermitRefusal::NotVerified(item.state));
        };
        if basis != PublishBasis::ExportedWithWarnings && has_unresolved(evidence) {
            return Err(PermitRefusal::UnresolvedProblems);
        }
        Ok(Self {
            item: item.id,
            basis,
        })
    }

    pub fn item(&self) -> ItemId {
        self.item
    }

    pub fn basis(&self) -> PublishBasis {
        self.basis
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::{Problem, Resolution};
    use crate::detection::ModelId;

    fn gap() -> CoverageEvidence {
        CoverageEvidence::unresolved(Problem::UnresolvedGap {
            track: 1,
            first: 30,
            last: 54,
        })
    }

    fn capped() -> CoverageEvidence {
        CoverageEvidence::unresolved(Problem::CappedCandidates {
            model: ModelId::Yolo5FaceN,
            frame: None,
            kept: 100,
            found: 259,
        })
    }

    fn analyzed(id: u64) -> Item {
        let mut item = Item::new(ItemId(id));
        item.finish_analysis().unwrap();
        item
    }

    fn render_and_verify(item: &mut Item) {
        item.start_render().unwrap();
        item.mark_verified().unwrap();
    }

    #[test]
    fn automatic_mode_publishes_items_without_problems_as_unreviewed() {
        let mut item = analyzed(1);
        item.route(ReviewMode::Off, &[]).unwrap();
        render_and_verify(&mut item);
        let permit = PublishPermit::issue(&item, &[]).unwrap();
        item.publish(permit).unwrap();
        assert_eq!(
            item.state(),
            ItemState::Published(PublishBasis::AutomaticUnreviewed)
        );
        assert!(PublishBasis::AutomaticUnreviewed.counts_as_normal_completion());
    }

    // P-1: a video gap or capped photo candidates keep the item in NeedsReview even with review
    // off, and nothing can be published.
    #[test]
    fn unresolved_problems_hold_the_item_in_review_with_review_off() {
        for evidence in [gap(), capped()] {
            let mut item = analyzed(1);
            item.route(ReviewMode::Off, std::slice::from_ref(&evidence))
                .unwrap();
            assert_eq!(item.state(), ItemState::NeedsReview);
            assert_eq!(
                item.approve_review(std::slice::from_ref(&evidence)),
                Err(TransitionError::UnresolvedProblems)
            );
            assert_eq!(
                PublishPermit::issue(&item, std::slice::from_ref(&evidence)),
                Err(PermitRefusal::NotVerified(ItemState::NeedsReview))
            );
        }
    }

    #[test]
    fn explicit_export_with_warnings_publishes_but_is_not_a_normal_completion() {
        let evidence = [gap(), capped()];
        let mut item = analyzed(1);
        item.route(ReviewMode::Off, &evidence).unwrap();
        item.export_with_warnings().unwrap();
        render_and_verify(&mut item);
        let permit = PublishPermit::issue(&item, &evidence).unwrap();
        item.publish(permit).unwrap();
        assert_eq!(
            item.state(),
            ItemState::Published(PublishBasis::ExportedWithWarnings)
        );
        assert!(!PublishBasis::ExportedWithWarnings.counts_as_normal_completion());
    }

    #[test]
    fn review_approval_needs_every_problem_covered_or_dismissed() {
        let mut evidence = [gap(), capped()];
        let mut item = analyzed(1);
        item.route(ReviewMode::Required, &evidence).unwrap();
        evidence[0].resolution = Resolution::CoveredByManualMask;
        assert_eq!(
            item.approve_review(&evidence),
            Err(TransitionError::UnresolvedProblems)
        );
        evidence[1].resolution = Resolution::Dismissed;
        item.approve_review(&evidence).unwrap();
        render_and_verify(&mut item);
        let permit = PublishPermit::issue(&item, &evidence).unwrap();
        item.publish(permit).unwrap();
        assert_eq!(item.state(), ItemState::Published(PublishBasis::Reviewed));
    }

    #[test]
    fn a_resumed_item_still_needs_approval_or_an_explicit_export() {
        let mut item = Item::awaiting_review(ItemId(4));
        assert!(item.start_render().is_err());
        assert_eq!(
            item.approve_review(&[gap()]),
            Err(TransitionError::UnresolvedProblems)
        );
        item.export_with_warnings().unwrap();
        assert_eq!(
            item.state(),
            ItemState::ReadyForRender(PublishBasis::ExportedWithWarnings)
        );
    }

    #[test]
    fn review_mode_required_sends_clean_items_to_review() {
        let mut item = analyzed(1);
        item.route(ReviewMode::Required, &[]).unwrap();
        assert_eq!(item.state(), ItemState::NeedsReview);
    }

    #[test]
    fn permit_is_refused_when_problems_appear_after_routing() {
        let mut item = analyzed(1);
        item.route(ReviewMode::Off, &[]).unwrap();
        render_and_verify(&mut item);
        assert_eq!(
            PublishPermit::issue(&item, &[gap()]),
            Err(PermitRefusal::UnresolvedProblems)
        );
    }

    #[test]
    fn permit_only_publishes_the_item_it_was_issued_for() {
        let mut a = analyzed(1);
        let mut b = analyzed(2);
        for item in [&mut a, &mut b] {
            item.route(ReviewMode::Off, &[]).unwrap();
            render_and_verify(item);
        }
        let permit_for_a = PublishPermit::issue(&a, &[]).unwrap();
        assert_eq!(
            b.publish(permit_for_a),
            Err(TransitionError::PermitMismatch)
        );
        assert_eq!(
            b.state(),
            ItemState::Verified(PublishBasis::AutomaticUnreviewed)
        );
    }

    #[test]
    fn cancelled_or_failed_items_get_no_permit() {
        let mut item = analyzed(1);
        item.route(ReviewMode::Off, &[]).unwrap();
        render_and_verify(&mut item);
        item.cancel().unwrap();
        assert_eq!(
            PublishPermit::issue(&item, &[]),
            Err(PermitRefusal::NotVerified(ItemState::Cancelled))
        );
        assert!(item.fail().is_err());
    }

    #[test]
    fn transitions_out_of_order_are_rejected() {
        let mut item = Item::new(ItemId(1));
        assert!(item.route(ReviewMode::Off, &[]).is_err());
        assert!(item.start_render().is_err());
        assert!(item.mark_verified().is_err());
        assert!(item.export_with_warnings().is_err());
        item.finish_analysis().unwrap();
        assert!(item.finish_analysis().is_err());
        assert_eq!(item.state(), ItemState::Analyzed);
    }

    #[test]
    fn every_non_terminal_state_can_fail_or_cancel() {
        let paths: [fn(&mut Item); 6] = [
            |_| {},
            |i| i.finish_analysis().unwrap(),
            |i| {
                i.finish_analysis().unwrap();
                i.route(ReviewMode::Required, &[]).unwrap();
            },
            |i| {
                i.finish_analysis().unwrap();
                i.route(ReviewMode::Off, &[]).unwrap();
            },
            |i| {
                i.finish_analysis().unwrap();
                i.route(ReviewMode::Off, &[]).unwrap();
                i.start_render().unwrap();
            },
            |i| {
                i.finish_analysis().unwrap();
                i.route(ReviewMode::Off, &[]).unwrap();
                render_and_verify(i);
            },
        ];
        for path in paths {
            for end in [Item::fail, Item::cancel] {
                let mut item = Item::new(ItemId(1));
                path(&mut item);
                assert!(!item.state().is_terminal());
                end(&mut item).unwrap();
                assert!(item.state().is_terminal());
                assert!(item.cancel().is_err());
            }
        }
    }
}
