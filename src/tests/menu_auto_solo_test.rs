//! #1155 — solo-owner group auto-registration: the pure decision core.
//!
//! `evaluate_solo_group` decides whether an unconfigured group gets the full
//! owner catalog automatically. Trigger rule per the issue: no humans other
//! than the bot owner; extra BOTS are ignored entirely. These tests pin that
//! rule so a future refactor cannot quietly start counting bots or
//! misclassifying the owner.

use crate::channels::telegram::menu_auto::{MemberView, SoloEval, evaluate_solo_group};

fn human(uid: i64) -> MemberView {
    MemberView {
        user_id: uid,
        is_bot: false,
    }
}

fn bot(uid: i64) -> MemberView {
    MemberView {
        user_id: uid,
        is_bot: true,
    }
}

const OWNER: i64 = 111;

#[test]
fn owner_plus_bots_is_eligible() {
    // The whole point of the trigger rule: any number of bots, zero humans
    // besides the owner, still registers.
    let members = vec![bot(1), bot(2), bot(3), human(OWNER), bot(4)];
    assert_eq!(evaluate_solo_group(&members, OWNER), SoloEval::Eligible);
}

#[test]
fn second_human_blocks_registration() {
    let members = vec![human(OWNER), bot(7), human(222)];
    assert_eq!(
        evaluate_solo_group(&members, OWNER),
        SoloEval::OtherHumans(vec![222])
    );
}

#[test]
fn other_humans_excludes_owner_and_bots() {
    let members = vec![human(OWNER), bot(1), human(222), human(333), bot(2)];
    assert_eq!(
        evaluate_solo_group(&members, OWNER),
        SoloEval::OtherHumans(vec![222, 333])
    );
}

#[test]
fn owner_absent_is_not_eligible() {
    // A group full of bots and strangers but no owner: registering an owner
    // scope would fail at the API and must not be attempted.
    let members = vec![bot(1), human(999)];
    assert_eq!(evaluate_solo_group(&members, OWNER), SoloEval::OwnerAbsent);
}

#[test]
fn empty_member_list_is_owner_absent() {
    // get_chat_administrators returned nothing usable: fail closed.
    assert_eq!(evaluate_solo_group(&[], OWNER), SoloEval::OwnerAbsent);
}

#[test]
fn owner_alone_is_eligible() {
    let members = vec![human(OWNER)];
    assert_eq!(evaluate_solo_group(&members, OWNER), SoloEval::Eligible);
}
