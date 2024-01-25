use winit::event_loop::EventLoopWindowTarget;
use yippee::yippee_test;

fn smoke(_elwt: &EventLoopWindowTarget<()>) {}
fn other_smoke(_elwt: &EventLoopWindowTarget<()>) {}

yippee_test!(smoke, other_smoke);
