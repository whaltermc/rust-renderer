#include "trace_benchmark.hpp"

#include <chrono>
#include <cstdlib>
#include <cstring>
#include <utility>
#include <dlfcn.h>

namespace mobilegl_trace {
namespace benchmark {
namespace {

using Clock = std::chrono::steady_clock;
using GlFinishFn = void (*)();

constexpr std::size_t kFrameReserve = 4096;

bool gEnabled = false;
bool gFinishEachFrame = false;
bool gResolvedGlFinish = false;
GlFinishFn gGlFinish = nullptr;
Clock::time_point gStart;
Clock::time_point gLastBoundary;
std::vector<double> gFrameMs;

// Same resolution order the glws layers use for Rust Renderer entry points: the replay driver
// already dlopen()ed the library with RTLD_GLOBAL before retrace started, so RTLD_NOLOAD
// finds that handle instead of loading a second copy. Strict renderer replay never falls back
// to the host GL implementation.
GlFinishFn ResolveGlFinish() {
    void *handle = nullptr;
    const char *library = std::getenv("RUST_RENDERER_TRACE_LIBRARY");
    if (library != nullptr && library[0] != '\0') {
        handle = dlopen(library, RTLD_NOW | RTLD_GLOBAL | RTLD_NOLOAD);
    }
    if (handle == nullptr) {
        handle = dlopen("libMobileGL.so", RTLD_NOW | RTLD_GLOBAL | RTLD_NOLOAD);
    }
    if (handle != nullptr) {
        void *symbol = dlsym(handle, "glFinish");
        if (symbol != nullptr) {
            return reinterpret_cast<GlFinishFn>(symbol);
        }
    }
    const char *strict = std::getenv("RUST_RENDERER_TRACE_STRICT");
    if (strict != nullptr && std::strcmp(strict, "1") == 0) {
        return nullptr;
    }
    return reinterpret_cast<GlFinishFn>(dlsym(RTLD_DEFAULT, "glFinish"));
}

} // namespace

void Begin(bool finishEachFrame) {
    gFrameMs.clear();
    gFrameMs.reserve(kFrameReserve);
    gFinishEachFrame = finishEachFrame;
    gResolvedGlFinish = false;
    gGlFinish = nullptr;
    gStart = Clock::now();
    gLastBoundary = gStart;
    gEnabled = true;
}

void OnFrameBoundary() {
    if (!gEnabled) {
        return;
    }
    if (gFinishEachFrame) {
        // Resolved on the first boundary rather than in Begin(): a context only exists once
        // the trace has created one, and glFinish before that would be pointless anyway.
        if (!gResolvedGlFinish) {
            gGlFinish = ResolveGlFinish();
            gResolvedGlFinish = true;
        }
        if (gGlFinish != nullptr) {
            gGlFinish();
        }
    }
    const Clock::time_point now = Clock::now();
    gFrameMs.push_back(std::chrono::duration<double, std::milli>(now - gLastBoundary).count());
    gLastBoundary = now;
}

Report End() {
    Report report;
    if (!gEnabled) {
        return report;
    }
    gEnabled = false;
    gFinishEachFrame = false;
    report.totalSeconds = std::chrono::duration<double>(Clock::now() - gStart).count();
    report.frameMs = std::move(gFrameMs);
    gFrameMs.clear();
    return report;
}

} // namespace benchmark
} // namespace mobilegl_trace
