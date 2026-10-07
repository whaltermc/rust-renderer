cmake_minimum_required(VERSION 3.16)
project(renderer-tool LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(OpenGL REQUIRED)

find_library(EGL_LIB NAMES EGL PATHS /usr/lib/x86_64-linux-gnu REQUIRED)
find_library(DL_LIB NAMES dl PATHS /usr/lib/x86_64-linux-gnu REQUIRED)

add_executable(renderer-tool main.cpp)
target_link_libraries(renderer-tool PRIVATE OpenGL::GL ${EGL_LIB} ${DL_LIB})
