use nabla_game;
use nabla_game::game::online::OnlineSession;
use nabla_game::render::util::RenderId;

#[test]
fn test_record_click_buffers_normal_clicks_in_order() {
    let mut session = OnlineSession::new(1);
    session.record_click(RenderId::Field0);
    session.record_click(RenderId::PlayerOne3);
    assert_eq!(session.take_outgoing(), vec![RenderId::Field0, RenderId::PlayerOne3]);
}

#[test]
fn test_record_click_excludes_confirm() {
    let mut session = OnlineSession::new(1);
    session.record_click(RenderId::Field0);
    session.record_click(RenderId::Confirm);
    assert_eq!(session.take_outgoing(), vec![RenderId::Field0]);
}

#[test]
fn test_record_click_cancel_discards_the_whole_buffer() {
    let mut session = OnlineSession::new(1);
    session.record_click(RenderId::Field0);
    session.record_click(RenderId::PlayerOne1);
    session.record_click(RenderId::Cancel);
    assert_eq!(session.take_outgoing(), Vec::<RenderId>::new());
}

#[test]
fn test_take_outgoing_clears_the_buffer() {
    let mut session = OnlineSession::new(1);
    session.record_click(RenderId::Field0);
    session.take_outgoing();
    assert_eq!(session.take_outgoing(), Vec::<RenderId>::new());
}
