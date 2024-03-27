use std::mem::{size_of, size_of_val};

use servo::gl;

type GlPtr = std::rc::Rc<dyn gl::Gl>;

const VS_SRC: &'static [&[u8]] = &[b"#version 300 es
layout (location = 0) in vec3 pos;

void main() {
gl_Position = vec4(pos.x, pos.y, pos.z, 1.0);
}"];

const FS_SRC: &'static [&[u8]] = &[b"#version 300 es
precision mediump float;
out vec4 final_color;

void main() {
final_color = vec4(1.0, 0.5, 0.2, 1.0);
}"];

type Vertex = [f32; 3];
const VERTICES: [Vertex; 3] = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]];

#[repr(C)]
/// Painter struct to handle gl bindings and rendering.
pub struct Painter {
    gl: GlPtr,
    program: gl::GLuint,
    buffer: gl::GLuint,
    // width: i32,
    // height: i32,
    destroyed: bool,
}

fn load_shader(gl: &GlPtr, shader_type: gl::GLenum, source: &[&[u8]]) -> Option<gl::GLuint> {
    let shader = gl.create_shader(shader_type);
    if shader == 0 {
        return None;
    }
    gl.shader_source(shader, source);
    gl.compile_shader(shader);
    let mut compiled = [0];
    unsafe {
        gl.get_shader_iv(shader, gl::COMPILE_STATUS, &mut compiled);
    }
    if compiled[0] == 0 {
        let log = gl.get_shader_info_log(shader);
        println!("{}", log);
        gl.delete_shader(shader);
        return None;
    }
    Some(shader)
}

fn init_buffer(gl: &GlPtr, program: gl::GLuint) -> Option<gl::GLuint> {
    // let elements: Vec<u16> = vec![
    //     3, 2, 0, 2, 0, 1, 0, 1, 4, 1, 4, 5, 1, 2, 5, 2, 5, 6, 2, 3, 6, 3, 6, 7, 3, 0, 7, 0, 7, 4,
    //     4, 5, 7, 5, 7, 6,
    // ];
    let buffers = gl.gen_buffers(2);
    let vbo = buffers[0];
    // let element_buffer = buffers[1];
    // let position_location = gl.get_attrib_location(program, "aPosition") as u32;
    // let color_location = gl.get_attrib_location(program, "aColor") as u32;
    let vao = gl.gen_vertex_arrays(1)[0];
    gl.bind_vertex_array(vao);
    // gl.enable_vertex_attrib_array(position_location);
    // gl.enable_vertex_attrib_array(color_location);
    gl.bind_buffer(gl::ARRAY_BUFFER, vbo);
    gl.buffer_data_untyped(
        gl::ARRAY_BUFFER,
        size_of_val(&VERTICES) as isize,
        VERTICES.as_ptr() as *const _,
        gl::STATIC_DRAW,
    );
    // gl.vertex_attrib_pointer(position_location, 3, gl::FLOAT, false, 24, 0);
    // gl.vertex_attrib_pointer(color_location, 3, gl::FLOAT, false, 24, 12);
    // gl.bind_buffer(gl::ELEMENT_ARRAY_BUFFER, element_buffer);
    // gl.buffer_data_untyped(
    //     gl::ELEMENT_ARRAY_BUFFER,
    //     2 * elements.len() as isize,
    //     elements.as_ptr() as *const _,
    //     gl::STATIC_DRAW,
    // );
    gl.vertex_attrib_pointer(
        0,
        3,
        gl::FLOAT,
        false,
        size_of::<Vertex>().try_into().unwrap(),
        0,
    );
    gl.bind_vertex_array(0);
    Some(vao)
}

impl Painter {
    /// Create a new Painter instance.
    pub fn new(gl: GlPtr) -> Painter {
        let v_shader = load_shader(&gl, gl::VERTEX_SHADER, VS_SRC).unwrap();
        let f_shader = load_shader(&gl, gl::FRAGMENT_SHADER, FS_SRC).unwrap();
        let program = gl.create_program();
        gl.attach_shader(program, v_shader);
        gl.attach_shader(program, f_shader);
        gl.link_program(program);
        gl.use_program(program);
        // let position_location = gl.get_attrib_location(program, "aPosition") as u32;
        // let color_location = gl.get_attrib_location(program, "aColor") as u32;
        // gl.enable_vertex_attrib_array(position_location);
        // gl.enable_vertex_attrib_array(color_location);
        let buffer = init_buffer(&gl, program).unwrap();
        // gl.clear_color(0.0, 0.0, 0.0, 1.0);
        gl.enable(gl::DEPTH_TEST);
        Painter {
            gl: gl,
            program: program,
            buffer: buffer,
            // width: width,
            // height: height,
            destroyed: false,
        }
    }
    /// Execute full screen drawing.
    pub fn draw(&self) {
        let gl = &self.gl;
        // gl.viewport(0, 0, self.width as i32, self.height as i32);
        // gl.clear(gl::COLOR_BUFFER_BIT);
        gl.use_program(self.program);
        gl.bind_vertex_array(self.buffer);
        gl.draw_arrays(gl::TRIANGLES, 0, 3);
        gl.bind_vertex_array(0);
    }
    unsafe fn destroy_gl(&self) {
        self.gl.delete_program(self.program);
        self.gl.delete_buffers(&[self.buffer]);
    }

    /// This function must be called before [`Painter`] is dropped, as [`Painter`] has some OpenGL objects
    /// that should be deleted.
    pub fn destroy(&mut self) {
        if !self.destroyed {
            unsafe {
                self.destroy_gl();
            }
            self.destroyed = true;
        }
    }
    fn assert_not_destroyed(&self) {
        assert!(!self.destroyed, "the painter has already been destroyed!");
    }
}

impl Drop for Painter {
    fn drop(&mut self) {
        if !self.destroyed {
            log::warn!(
                "You forgot to call destroy() on the painter. Resources will leak!"
            );
        }
    }
}

