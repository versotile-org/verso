use verso::verso_test;
use winit::event_loop::EventLoopWindowTarget;

fn smoke(_elwt: &EventLoopWindowTarget<()>) {}
fn other_smoke(_elwt: &EventLoopWindowTarget<()>) {}

verso_test!(smoke, other_smoke);
