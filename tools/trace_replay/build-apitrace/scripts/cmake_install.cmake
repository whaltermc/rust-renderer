# Install script for directory: /workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts

# Set the install prefix
if(NOT DEFINED CMAKE_INSTALL_PREFIX)
  set(CMAKE_INSTALL_PREFIX "/usr/local")
endif()
string(REGEX REPLACE "/$" "" CMAKE_INSTALL_PREFIX "${CMAKE_INSTALL_PREFIX}")

# Set the install configuration name.
if(NOT DEFINED CMAKE_INSTALL_CONFIG_NAME)
  if(BUILD_TYPE)
    string(REGEX REPLACE "^[^A-Za-z0-9_]+" ""
           CMAKE_INSTALL_CONFIG_NAME "${BUILD_TYPE}")
  else()
    set(CMAKE_INSTALL_CONFIG_NAME "Release")
  endif()
  message(STATUS "Install configuration: \"${CMAKE_INSTALL_CONFIG_NAME}\"")
endif()

# Set the component getting installed.
if(NOT CMAKE_INSTALL_COMPONENT)
  if(COMPONENT)
    message(STATUS "Install component: \"${COMPONENT}\"")
    set(CMAKE_INSTALL_COMPONENT "${COMPONENT}")
  else()
    set(CMAKE_INSTALL_COMPONENT)
  endif()
endif()

# Install shared libraries without execute permission?
if(NOT DEFINED CMAKE_INSTALL_SO_NO_EXE)
  set(CMAKE_INSTALL_SO_NO_EXE "1")
endif()

# Is this installation the result of a crosscompile?
if(NOT DEFINED CMAKE_CROSSCOMPILING)
  set(CMAKE_CROSSCOMPILING "FALSE")
endif()

# Set default install directory permissions.
if(NOT DEFINED CMAKE_OBJDUMP)
  set(CMAKE_OBJDUMP "/usr/bin/objdump")
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib/apitrace/scripts" TYPE PROGRAM FILES
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/convert.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/jsondiff.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/jsonextractimages.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/leaks.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/profileshader.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/retracediff.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/snapdiff.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/tracecheck.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/tracediff.py"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/unpickle.py"
    )
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib/apitrace/scripts" TYPE FILE FILES
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/apitrace.PIXExp"
    "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/highlight.py"
    )
endif()

if(CMAKE_INSTALL_COMPONENT STREQUAL "Unspecified" OR NOT CMAKE_INSTALL_COMPONENT)
  file(INSTALL DESTINATION "${CMAKE_INSTALL_PREFIX}/lib/apitrace/scripts" TYPE FILE FILES "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/scripts/apitrace.PIXExp")
endif()

