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
final_color = vec4(1.0, 0.5, 0.2, 0.5);
}"];

type Vertex = [f32; 3];
const VERTICES: [Vertex; 6] = [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [-1.0, -1.0, 0.0], [1.0, 1.0, 0.0]];

#[repr(C)]
/// Painter struct to handle gl bindings and rendering.
pub struct Painter {
    gl: GlPtr,
    program: gl::GLuint,
    buffer: gl::GLuint,
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
    let vao = gl.gen_vertex_arrays(1)[0];
    gl.bind_vertex_array(vao);

    let vbo = gl.gen_buffers(1)[0];
    gl.bind_buffer(gl::ARRAY_BUFFER, vbo);
    gl.buffer_data_untyped(
        gl::ARRAY_BUFFER,
        size_of_val(&VERTICES) as isize,
        VERTICES.as_ptr().cast(),
        gl::STATIC_DRAW,
    );

    gl.vertex_attrib_pointer(
        0,
        3,
        gl::FLOAT,
        false,
        size_of::<Vertex>().try_into().unwrap(),
        0,
    );
    gl.enable_vertex_attrib_array(0);
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
        let buffer = init_buffer(&gl, program).unwrap();
        Painter {
            gl: gl,
            program: program,
            buffer: buffer,
            destroyed: false,
        }
    }
    /// Execute full screen drawing.
    pub fn draw(&self) {
        let gl = &self.gl;
        // gl.clear(gl::COLOR_BUFFER_BIT);
        gl.use_program(self.program);
        gl.bind_vertex_array(self.buffer);
        gl.draw_arrays(gl::TRIANGLES, 0, 6);
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
            log::warn!("You forgot to call destroy() on the painter. Resources will leak!");
        }
    }
}
