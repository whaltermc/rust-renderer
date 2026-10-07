#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <string>
#include <vector>
#include <fstream>
#include <sstream>
#include <algorithm>
#include <map>

#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <dlfcn.h>

/* ---- GL types and enums ---- */
typedef unsigned int GLenum;
typedef unsigned char GLboolean;
typedef unsigned int GLbitfield;
typedef void GLvoid;
typedef signed char GLbyte;
typedef short GLshort;
typedef int GLint;
typedef int GLsizei;
typedef unsigned char GLubyte;
typedef unsigned short GLushort;
typedef unsigned int GLuint;
typedef float GLfloat;
typedef double GLdouble;
typedef ptrdiff_t GLsizeiptr;
typedef ptrdiff_t GLintptr;
typedef char GLchar;

#define GL_FALSE 0
#define GL_TRUE 1
#define GL_TRIANGLES 0x0004
#define GL_UNSIGNED_BYTE 0x1401
#define GL_FLOAT 0x1406
#define GL_RGBA 0x1908
#define GL_NEAREST 0x2600
#define GL_TEXTURE_MAG_FILTER 0x2800
#define GL_TEXTURE_MIN_FILTER 0x2801
#define GL_RGBA8 0x8058
#define GL_TEXTURE_2D 0x0DE1
#define GL_TEXTURE0 0x84C0
#define GL_PACK_ALIGNMENT 0x0D05
#define GL_FRAMEBUFFER 0x8D40
#define GL_COLOR_ATTACHMENT0 0x8CE0
#define GL_FRAMEBUFFER_COMPLETE 0x8CD5
#define GL_ARRAY_BUFFER 0x8892
#define GL_STATIC_DRAW 0x88E4
#define GL_VERTEX_SHADER 0x8B31
#define GL_FRAGMENT_SHADER 0x8B30
#define GL_COMPILE_STATUS 0x8B81
#define GL_INFO_LOG_LENGTH 0x8B84
#define GL_LINK_STATUS 0x8B82
#define GL_COLOR_BUFFER_BIT 0x00004000
#define GL_DEPTH_BUFFER_BIT 0x00000100
#define GL_DEPTH_TEST 0x0B71
#define GL_BLEND 0x0BE2
#define GL_SCISSOR_TEST 0x0C11
#define GL_LESS 0x0201
#define GL_SRC_ALPHA 0x0302
#define GL_ONE_MINUS_SRC_ALPHA 0x0303
#define GL_CULL_FACE 0x0B44
#define GL_BACK 0x0405

/* ---- Function pointer types ---- */
#define DECL(ret, name, args) typedef ret (*PFN_##name) args;
DECL(void, glGenFramebuffers, (GLsizei, GLuint*))
DECL(void, glBindFramebuffer, (GLenum, GLuint))
DECL(GLenum, glCheckFramebufferStatus, (GLenum))
DECL(void, glGenTextures, (GLsizei, GLuint*))
DECL(void, glBindTexture, (GLenum, GLuint))
DECL(void, glTexStorage2D, (GLenum, GLint, GLenum, GLsizei, GLsizei))
DECL(void, glFramebufferTexture2D, (GLenum, GLenum, GLenum, GLuint, GLint))
DECL(GLuint, glCreateShader, (GLenum))
DECL(void, glShaderSource, (GLuint, GLsizei, const GLchar* const*, const GLint*))
DECL(void, glCompileShader, (GLuint))
DECL(void, glGetShaderiv, (GLuint, GLenum, GLint*))
DECL(void, glGetShaderInfoLog, (GLuint, GLsizei, GLsizei*, GLchar*))
DECL(GLuint, glCreateProgram, (void))
DECL(void, glAttachShader, (GLuint, GLuint))
DECL(void, glLinkProgram, (GLuint))
DECL(void, glGetProgramiv, (GLuint, GLenum, GLint*))
DECL(void, glGetProgramInfoLog, (GLuint, GLsizei, GLsizei*, GLchar*))
DECL(void, glDeleteProgram, (GLuint))
DECL(void, glUseProgram, (GLuint))
DECL(GLint, glGetUniformLocation, (GLuint, const GLchar*))
DECL(void, glUniform1i, (GLint, GLint))
DECL(void, glActiveTexture, (GLenum))
DECL(void, glGenVertexArrays, (GLsizei, GLuint*))
DECL(void, glBindVertexArray, (GLuint))
DECL(void, glGenBuffers, (GLsizei, GLuint*))
DECL(void, glBindBuffer, (GLenum, GLuint))
DECL(void, glBufferData, (GLenum, GLsizeiptr, const void*, GLenum))
DECL(GLint, glGetAttribLocation, (GLuint, const GLchar*))
DECL(void, glEnableVertexAttribArray, (GLuint))
DECL(void, glVertexAttribPointer, (GLuint, GLint, GLenum, GLboolean, GLsizei, const void*))
DECL(void, glViewport, (GLint, GLint, GLsizei, GLsizei))
DECL(void, glClearColor, (GLfloat, GLfloat, GLfloat, GLfloat))
DECL(void, glClear, (GLbitfield))
DECL(void, glDrawArrays, (GLenum, GLint, GLsizei))
DECL(void, glPixelStorei, (GLenum, GLint))
DECL(void, glReadPixels, (GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, void*))
DECL(GLenum, glGetError, (void))
DECL(const GLubyte*, glGetString, (GLenum))
DECL(void, glEnable, (GLenum))
DECL(void, glDisable, (GLenum))
DECL(void, glBlendFunc, (GLenum, GLenum))
DECL(void, glDepthFunc, (GLenum))
DECL(void, glDepthMask, (GLboolean))
DECL(void, glCullFace, (GLenum))
DECL(void, glDeleteShader, (GLuint))
DECL(void, glDeleteTextures, (GLsizei, const GLuint*))
DECL(void, glDeleteBuffers, (GLsizei, const GLuint*))
DECL(void, glDeleteVertexArrays, (GLsizei, const GLuint*))
DECL(void, glTexImage2D, (GLenum, GLint, GLint, GLsizei, GLsizei, GLint, GLenum, GLenum, const void*))
DECL(void, glTexParameteri, (GLenum, GLenum, GLint))

typedef EGLDisplay (*PFN_eglGetPlatformDisplayEXT)(EGLenum, void*, const EGLint*);

#define LOAD(var, sym) \
    var = (PFN_##var)getproc(sym); \
    if (!var) { fprintf(stderr, "missing %s\n", sym); return false; }

/* ---- Globals ---- */
static void* g_lib = nullptr;
static void* (*g_getproc)(const char*) = nullptr;

static PFN_glGenFramebuffers glGenFramebuffers;
static PFN_glBindFramebuffer glBindFramebuffer;
static PFN_glCheckFramebufferStatus glCheckFramebufferStatus;
static PFN_glGenTextures glGenTextures;
static PFN_glBindTexture glBindTexture;
static PFN_glTexStorage2D glTexStorage2D;
static PFN_glFramebufferTexture2D glFramebufferTexture2D;
static PFN_glCreateShader glCreateShader;
static PFN_glShaderSource glShaderSource;
static PFN_glCompileShader glCompileShader;
static PFN_glGetShaderiv glGetShaderiv;
static PFN_glGetShaderInfoLog glGetShaderInfoLog;
static PFN_glCreateProgram glCreateProgram;
static PFN_glAttachShader glAttachShader;
static PFN_glLinkProgram glLinkProgram;
static PFN_glGetProgramiv glGetProgramiv;
static PFN_glGetProgramInfoLog glGetProgramInfoLog;
static PFN_glDeleteProgram glDeleteProgram;
static PFN_glUseProgram glUseProgram;
static PFN_glGetUniformLocation glGetUniformLocation;
static PFN_glUniform1i glUniform1i;
static PFN_glActiveTexture glActiveTexture;
static PFN_glGenVertexArrays glGenVertexArrays;
static PFN_glBindVertexArray glBindVertexArray;
static PFN_glGenBuffers glGenBuffers;
static PFN_glBindBuffer glBindBuffer;
static PFN_glBufferData glBufferData;
static PFN_glGetAttribLocation glGetAttribLocation;
static PFN_glEnableVertexAttribArray glEnableVertexAttribArray;
static PFN_glVertexAttribPointer glVertexAttribPointer;
static PFN_glViewport glViewport;
static PFN_glClearColor glClearColor;
static PFN_glClear glClear;
static PFN_glDrawArrays glDrawArrays;
static PFN_glPixelStorei glPixelStorei;
static PFN_glReadPixels glReadPixels;
static PFN_glGetError glGetError;
static PFN_glGetString glGetString;
static PFN_glEnable glEnable;
static PFN_glDisable glDisable;
static PFN_glBlendFunc glBlendFunc;
static PFN_glDepthFunc glDepthFunc;
static PFN_glDepthMask glDepthMask;
static PFN_glCullFace glCullFace;
static PFN_glDeleteShader glDeleteShader;
static PFN_glDeleteTextures glDeleteTextures;
static PFN_glDeleteBuffers glDeleteBuffers;
static PFN_glDeleteVertexArrays glDeleteVertexArrays;
static PFN_glTexImage2D glTexImage2D;
static PFN_glTexParameteri glTexParameteri;

struct FrameResult {
    std::string backend;
    std::string renderer;
    std::string gl_version;
    bool can_render;
    std::vector<uint8_t> pixels;
    std::vector<uint8_t> filtered;
    std::vector<std::string> compile_log;
    bool passed;
};

bool load_gl(const char* libpath) {
    g_lib = dlopen(libpath, RTLD_NOW | RTLD_LOCAL);
    if (!g_lib) {
        fprintf(stderr, "dlopen(%s): %s\n", libpath, dlerror());
        return false;
    }
    g_getproc = (void*(*)(const char*))dlsym(g_lib, "glGetProcAddress");
    if (!g_getproc) g_getproc = (void*(*)(const char*))dlsym(g_lib, "glXGetProcAddress");
    if (!g_getproc) {
        fprintf(stderr, "renderer does not export getProcAddress\n");
        return false;
    }

    auto getproc = [](const char* name) -> void* {
        return g_getproc(name);
    };

    LOAD(glGenFramebuffers, "glGenFramebuffers");
    LOAD(glBindFramebuffer, "glBindFramebuffer");
    LOAD(glCheckFramebufferStatus, "glCheckFramebufferStatus");
    LOAD(glGenTextures, "glGenTextures");
    LOAD(glBindTexture, "glBindTexture");
    LOAD(glTexStorage2D, "glTexStorage2D");
    LOAD(glFramebufferTexture2D, "glFramebufferTexture2D");
    LOAD(glCreateShader, "glCreateShader");
    LOAD(glShaderSource, "glShaderSource");
    LOAD(glCompileShader, "glCompileShader");
    LOAD(glGetShaderiv, "glGetShaderiv");
    LOAD(glGetShaderInfoLog, "glGetShaderInfoLog");
    LOAD(glCreateProgram, "glCreateProgram");
    LOAD(glAttachShader, "glAttachShader");
    LOAD(glLinkProgram, "glLinkProgram");
    LOAD(glGetProgramiv, "glGetProgramiv");
    LOAD(glGetProgramInfoLog, "glGetProgramInfoLog");
    LOAD(glDeleteProgram, "glDeleteProgram");
    LOAD(glUseProgram, "glUseProgram");
    LOAD(glGetUniformLocation, "glGetUniformLocation");
    LOAD(glUniform1i, "glUniform1i");
    LOAD(glActiveTexture, "glActiveTexture");
    LOAD(glGenVertexArrays, "glGenVertexArrays");
    LOAD(glBindVertexArray, "glBindVertexArray");
    LOAD(glGenBuffers, "glGenBuffers");
    LOAD(glBindBuffer, "glBindBuffer");
    LOAD(glBufferData, "glBufferData");
    LOAD(glGetAttribLocation, "glGetAttribLocation");
    LOAD(glEnableVertexAttribArray, "glEnableVertexAttribArray");
    LOAD(glVertexAttribPointer, "glVertexAttribPointer");
    LOAD(glViewport, "glViewport");
    LOAD(glClearColor, "glClearColor");
    LOAD(glClear, "glClear");
    LOAD(glDrawArrays, "glDrawArrays");
    LOAD(glPixelStorei, "glPixelStorei");
    LOAD(glReadPixels, "glReadPixels");
    LOAD(glGetError, "glGetError");
    LOAD(glGetString, "glGetString");
    LOAD(glEnable, "glEnable");
    LOAD(glDisable, "glDisable");
    LOAD(glBlendFunc, "glBlendFunc");
    LOAD(glDepthFunc, "glDepthFunc");
    LOAD(glDepthMask, "glDepthMask");
    LOAD(glCullFace, "glCullFace");
    LOAD(glDeleteShader, "glDeleteShader");
    LOAD(glDeleteTextures, "glDeleteTextures");
    LOAD(glDeleteBuffers, "glDeleteBuffers");
    LOAD(glDeleteVertexArrays, "glDeleteVertexArrays");
    LOAD(glTexImage2D, "glTexImage2D");
    LOAD(glTexParameteri, "glTexParameteri");

    return true;
}

std::string safe_str(const GLubyte* p) {
    if (!p) return "(null)";
    return std::string(reinterpret_cast<const char*>(p));
}

std::string shader_log(GLuint id) {
    GLint len = 0;
    glGetShaderiv(id, GL_INFO_LOG_LENGTH, &len);
    if (len <= 1) return {};
    std::vector<char> buf(len);
    GLsizei written = 0;
    glGetShaderInfoLog(id, len, &written, buf.data());
    return std::string(buf.data(), written);
}

std::string program_log(GLuint id) {
    GLint len = 0;
    glGetProgramiv(id, GL_INFO_LOG_LENGTH, &len);
    if (len <= 1) return {};
    std::vector<char> buf(len);
    GLsizei written = 0;
    glGetProgramInfoLog(id, len, &written, buf.data());
    return std::string(buf.data(), written);
}

std::vector<uint8_t> filter_pixels(const uint8_t* rgba, int w, int h) {
    int n = w * h * 4;
    std::vector<uint8_t> out(rgba, rgba + n);

    // Grayscale
    for (int i = 0; i < n; i += 4) {
        uint8_t r = out[i], g = out[i+1], b = out[i+2];
        uint8_t lum = (r * 299 + g * 587 + b * 114) / 1000;
        out[i] = out[i+1] = out[i+2] = lum;
    }

    // Mean / std per channel
    double sum[4] = {0,0,0,0};
    int pixels = w * h;
    for (int i = 0; i < n; i++) sum[i % 4] += out[i];
    double mean[4] = { sum[0]/pixels, sum[1]/pixels, sum[2]/pixels, sum[3]/pixels };

    double var[4] = {0,0,0,0};
    for (int i = 0; i < n; i++) {
        double d = out[i] - mean[i % 4];
        var[i % 4] += d * d;
    }
    double std[4] = { sqrt(var[0]/pixels), sqrt(var[1]/pixels), sqrt(var[2]/pixels), sqrt(var[3]/pixels) };

    for (int i = 0; i < n; i += 4) {
        for (int c = 0; c < 3; c++) {
            double v = out[i+c];
            double m = mean[c];
            double s = std[c];
            double z = s > 0 ? ((v - m) * 100 / s) : 0;
            z = std::max(-100.0, std::min(100.0, z));
            out[i+c] = static_cast<uint8_t>(std::max(0.0, std::min(255.0, m + z * s / 100.0)));
        }
    }
    return out;
}

bool write_ppm(const char* path, const uint8_t* rgba, int w, int h) {
    std::ofstream f(path, std::ios::binary);
    if (!f) return false;
    f << "P6\n" << w << " " << h << "\n255\n";
    for (int i = 0; i < w * h * 4; i += 4) {
        f.put(rgba[i]);
        f.put(rgba[i+1]);
        f.put(rgba[i+2]);
    }
    return true;
}

FrameResult run_backend(const char* label, const char* backend_env) {
    FrameResult res;
    res.backend = label;
    res.can_render = true;
    res.passed = false;

    if (backend_env) setenv("RENDERER_BACKEND", backend_env, 1);

    glViewport(0, 0, 64, 64);
    glClearColor(0.0, 0.0, 0.0, 1.0);
    glClear(GL_COLOR_BUFFER_BIT);
    glEnable(GL_DEPTH_TEST);
    glDepthFunc(GL_LESS);
    glEnable(GL_BLEND);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glCullFace(GL_BACK);
    glEnable(GL_CULL_FACE);

    const char* vs_src =
        "#version 120\n"
        "attribute vec3 aPos;\n"
        "attribute vec3 aCol;\n"
        "varying vec3 vCol;\n"
        "void main() {\n"
        "  vCol = aCol;\n"
        "  gl_Position = vec4(aPos, 1.0);\n"
        "}\n";

    const char* fs_src =
        "#version 120\n"
        "varying vec3 vCol;\n"
        "void main() {\n"
        "  gl_FragColor = vec4(vCol, 1.0);\n"
        "}\n";

    GLuint vs = glCreateShader(GL_VERTEX_SHADER);
    GLuint fs = glCreateShader(GL_FRAGMENT_SHADER);
    GLuint prog = 0;

    if (vs) {
        glShaderSource(vs, 1, &vs_src, nullptr);
        glCompileShader(vs);
        GLint ok = 0;
        glGetShaderiv(vs, GL_COMPILE_STATUS, &ok);
        if (!ok) res.compile_log.push_back("VS: " + shader_log(vs));
    } else {
        res.compile_log.push_back("glCreateShader returned 0");
    }

    if (fs) {
        glShaderSource(fs, 1, &fs_src, nullptr);
        glCompileShader(fs);
        GLint ok = 0;
        glGetShaderiv(fs, GL_COMPILE_STATUS, &ok);
        if (!ok) res.compile_log.push_back("FS: " + shader_log(fs));
    } else {
        res.compile_log.push_back("glCreateShader returned 0");
    }

    if (vs && fs) {
        prog = glCreateProgram();
        if (prog) {
            glAttachShader(prog, vs);
            glAttachShader(prog, fs);
            glLinkProgram(prog);
            GLint ok = 0;
            glGetProgramiv(prog, GL_LINK_STATUS, &ok);
            if (!ok) res.compile_log.push_back("link: " + program_log(prog));
        }
    }

    res.renderer = safe_str(glGetString(0x1F01));
    res.gl_version = safe_str(glGetString(0x1F02));
    res.passed = res.compile_log.empty();

    if (prog && res.passed) {
        glUseProgram(prog);

        GLuint white = 0;
        glGenTextures(1, &white);
        glBindTexture(GL_TEXTURE_2D, white);
        uint8_t px[4] = {255,255,255,255};
        glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, 1, 1, 0, GL_RGBA, GL_UNSIGNED_BYTE, px);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D, white);

        GLuint vao = 0, vbo = 0;
        glGenVertexArrays(1, &vao);
        glBindVertexArray(vao);
        glGenBuffers(1, &vbo);
        glBindBuffer(GL_ARRAY_BUFFER, vbo);
        float verts[18] = {
            -0.8f, -0.8f, 0.0f,  1.0f, 0.0f, 0.0f,
             0.8f, -0.8f, 0.0f,  0.0f, 1.0f, 0.0f,
             0.0f,  0.8f, 0.0f,  0.0f, 0.0f, 1.0f,
        };
        glBufferData(GL_ARRAY_BUFFER, sizeof(verts), verts, GL_STATIC_DRAW);

        GLint apos = glGetAttribLocation(prog, "aPos");
        GLint acol = glGetAttribLocation(prog, "aCol");
        if (apos >= 0 && acol >= 0) {
            glEnableVertexAttribArray(apos);
            glVertexAttribPointer(apos, 3, GL_FLOAT, GL_FALSE, 24, nullptr);
            glEnableVertexAttribArray(acol);
            glVertexAttribPointer(acol, 3, GL_FLOAT, GL_FALSE, 24, reinterpret_cast<const void*>(12));
        }

        glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
        glDrawArrays(GL_TRIANGLES, 0, 3);

        res.pixels.resize(64 * 64 * 4);
        glPixelStorei(GL_PACK_ALIGNMENT, 1);
        glReadPixels(0, 0, 64, 64, GL_RGBA, GL_UNSIGNED_BYTE, res.pixels.data());
        res.filtered = filter_pixels(res.pixels.data(), 64, 64);

        glDeleteBuffers(1, &vbo);
        glDeleteVertexArrays(1, &vao);
        glDeleteTextures(1, &white);
    }

    if (prog) glDeleteProgram(prog);
    if (vs) glDeleteShader(vs);
    if (fs) glDeleteShader(fs);

    return res;
}

double psnr(const uint8_t* a, const uint8_t* b, size_t n) {
    double mse = 0;
    for (size_t i = 0; i < n; i++) {
        double d = static_cast<double>(a[i]) - static_cast<double>(b[i]);
        mse += d * d;
    }
    mse /= n;
    if (mse == 0.0) return 1e18;
    return 20.0 * log10(255.0 / sqrt(mse));
}

int main(int argc, char** argv) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <path/to/librust_gl.so> [--output-dir <dir>]\n", argv[0]);
        return 1;
    }

    const char* so_path = argv[1];
    std::string output_dir = ".";
    for (int i = 2; i < argc; i++) {
        if (strcmp(argv[i], "--output-dir") == 0 && i + 1 < argc) {
            output_dir = argv[++i];
        }
    }

    if (!load_gl(so_path)) return 1;

    EGLDisplay dpy = EGL_NO_DISPLAY;
    auto eglGetPlatformDisplayEXT = (PFN_eglGetPlatformDisplayEXT)eglGetProcAddress("eglGetPlatformDisplayEXT");
    if (eglGetPlatformDisplayEXT) {
        dpy = eglGetPlatformDisplayEXT(EGL_PLATFORM_SURFACELESS_MESA, EGL_DEFAULT_DISPLAY, nullptr);
    }
    if (dpy == EGL_NO_DISPLAY) dpy = eglGetDisplay(EGL_DEFAULT_DISPLAY);
    if (dpy == EGL_NO_DISPLAY || !eglInitialize(dpy, nullptr, nullptr)) {
        fprintf(stderr, "no EGL display\n");
        return 1;
    }

    EGLint cfg_attrs[] = { EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT, EGL_NONE };
    EGLConfig cfg;
    EGLint ncfg = 0;
    eglChooseConfig(dpy, cfg_attrs, &cfg, 1, &ncfg);
    eglBindAPI(EGL_OPENGL_ES_API);
    EGLint ctx_attrs[] = { EGL_CONTEXT_MAJOR_VERSION, 3, EGL_CONTEXT_MINOR_VERSION, 0, EGL_NONE };
    EGLContext ctx = eglCreateContext(dpy, cfg, EGL_NO_CONTEXT, ctx_attrs);
    eglMakeCurrent(dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, ctx);

    std::vector<FrameResult> frames;
    frames.push_back(run_backend("es", "gles"));
    frames.push_back(run_backend("vk", "vulkan"));

    printf("\n=== Output filtering ===\n");
    for (auto& f : frames) {
        printf("backend=%s renderer=%s gl_version=%s passed=%d fail=",
               f.backend.c_str(), f.renderer.c_str(), f.gl_version.c_str(), f.passed);
        if (!f.compile_log.empty()) {
            for (auto& s : f.compile_log) printf("%s; ", s.c_str());
        }
        printf("\n");
        std::string ppm = output_dir + "/" + f.backend + "_filtered.ppm";
        write_ppm(ppm.c_str(), f.filtered.data(), 64, 64);
        printf("  filtered=%s\n", ppm.c_str());
    }

    if (frames.size() == 2) {
        double score = psnr(frames[0].filtered.data(), frames[1].filtered.data(), frames[0].filtered.size());
        printf("\n=== Backend comparison ===\n");
        printf("%s vs %s: PSNR=%.2f\n", frames[0].backend.c_str(), frames[1].backend.c_str(), score);
        if (score > 40.0 || std::isinf(score)) printf("PASS: backends agree\n");
        else printf("FAIL: backends diverge\n");
    }

    std::string json_path = output_dir + "/report.json";
    std::ofstream json(json_path);
    json << "[\n";
    for (size_t i = 0; i < frames.size(); i++) {
        auto& f = frames[i];
        if (i) json << ",\n";
        json << "  {\n";
        json << "    \"backend\": \"" << f.backend << "\",\n";
        json << "    \"renderer\": \"" << f.renderer << "\",\n";
        json << "    \"gl_version\": \"" << f.gl_version << "\",\n";
        json << "    \"can_render\": " << f.can_render << ",\n";
        json << "    \"passed\": " << f.passed << ",\n";
        json << "    \"compile_log\": [\n";
        for (size_t j = 0; j < f.compile_log.size(); j++) {
            if (j) json << ",\n";
            json << "      \"" << f.compile_log[j] << "\"";
        }
        json << "\n    ]\n  }";
    }
    json << "\n]\n";
    printf("\nreport: %s\n", json_path.c_str());

    eglDestroyContext(dpy, ctx);
    eglTerminate(dpy);
    dlclose(g_lib);
    return 0;
}
