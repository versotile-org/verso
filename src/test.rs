/// Simple test harness for testing `verso`.
///
/// Add your test to `Cargo.toml` with the `harness = false` option. This will prevent Rust's default test harness from running your test.
///
/// ```toml
/// [[test]]
/// name = "my_test"
/// harness = false
/// ```
///
/// Then, in your test, use the `verso_test!` macro to run your tests. The tests must be functions that take an `EventLoopWindowTarget`.
///
/// ```rust
/// use verso::verso_test;
/// use verso::winit::event_loop::EventLoopWindowTarget;
///
/// fn my_test(elwt: &EventLoopWindowTarget<()>) {
///     // ...
/// }
///
/// fn other_test(elwt: &EventLoopWindowTarget<()>) {
///     // ...
/// }
///
/// verso_test!(my_test, other_test);
/// ```
#[macro_export]
macro_rules! verso_test {
    ($($test:expr),*) => {
        fn main() -> Result<(), Box<dyn std::error::Error>> {
            const TESTS: &[$crate::test::__private::VersoBasedTest] = &[$(
                $crate::__verso_test_internal_collect_test!($test)
            ),*];

            $crate::test::__private::run(TESTS, ());
            Ok(())
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __verso_test_internal_collect_test {
    ($name:expr) => {
        $crate::test::__private::VersoBasedTest {
            name: stringify!($name),
            function: $crate::test::__private::TestFunction::Oneoff($name),
        }
    };
}

#[doc(hidden)]
// This module is forked from winit-test.
pub mod __private {
    use winit::event_loop::EventLoop;
    pub use winit::event_loop::EventLoopWindowTarget;

    use owo_colors::OwoColorize;
    use std::any::Any;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::time::Instant;
    use winit::window::WindowBuilder;

    use crate::{Status, Verso};

    pub type Context = ();

    struct State {
        passed: i32,
        panics: Vec<(&'static str, Box<dyn Any + Send>)>,
        start: Instant,
        run: bool,
        code: i32,
    }

    /// Run a set of tests using a `winit` context.
    pub fn run(tests: &'static [VersoBasedTest], _ctx: Context) {
        // Create a new event loop and obtain a window target.
        let event_loop = EventLoop::new().expect("Failed to build event loop");
        let window = WindowBuilder::new()
            .with_visible(false)
            .build(&event_loop)
            .expect("Failed to create winit window");
        let mut verso = Verso::new(window, event_loop.create_proxy());

        println!("\nRunning {} tests...", tests.len());
        let mut state = State {
            passed: 0,
            panics: vec![],
            start: Instant::now(),
            run: false,
            code: 0,
        };

        // Run the tests.
        event_loop
            .run(move |event, elwt| match verso.run(event, elwt) {
                Status::LoadComplete => {
                    run_internal(tests, &mut state, elwt);
                    verso.shutdown();
                }
                Status::Shutdown => elwt.exit(),
                _ => (),
            })
            .expect("Event loop failed to run");
    }

    /// Run a set of tests using a `winit` context.
    fn run_internal(
        tests: &'static [VersoBasedTest],
        state: &mut State,
        elwt: &EventLoopWindowTarget<()>,
    ) {
        if state.run {
            return;
        }
        state.run = true;

        for test in tests {
            print!("test {} ... ", test.name);

            match test.function {
                TestFunction::Oneoff(f) => match catch_unwind(AssertUnwindSafe(move || f(elwt))) {
                    Ok(()) => {
                        println!("{}", "ok".green());
                        state.passed += 1;
                    }

                    Err(e) => {
                        println!("{}", "FAILED".red());
                        state.panics.push((test.name, e));
                    }
                },
            }
        }

        let failures = state.panics.len();
        println!();
        if !state.panics.is_empty() {
            println!("failures:\n");
            for (name, e) in state.panics.drain(..) {
                println!("---- {} panic ----", name);

                if let Some(s) = e.downcast_ref::<&'static str>() {
                    println!("{}", s.red());
                } else if let Some(s) = e.downcast_ref::<String>() {
                    println!("{}", s.red());
                } else {
                    println!("{}", "unknown panic type".red());
                }

                println!();
            }

            print!("test result: {}", "FAILED".red());
        } else {
            print!("test result: {}", "ok".green());
        }

        let elapsed = state.start.elapsed();
        println!(
            ". {} passed; {} failed; finished in {:?}",
            state.passed, failures, elapsed
        );

        state.code = if failures == 0 { 0 } else { 1 };
    }

    pub struct VersoBasedTest {
        pub name: &'static str,
        pub function: TestFunction,
    }

    pub enum TestFunction {
        Oneoff(fn(&EventLoopWindowTarget<()>)),
    }
}
