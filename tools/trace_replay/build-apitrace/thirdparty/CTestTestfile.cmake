# CMake generated Testfile for 
# Source directory: /workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty
# Build directory: /workspaces/rust-renderer/tools/trace_replay/build-apitrace/thirdparty
# 
# This file includes the relevant testing commands required for 
# testing this directory and lists subdirectories to be tested as well.
add_test([=[libbacktrace_btest]=] "/home/codespace/.python/current/bin/python3" "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/support/libbacktrace/closefds.py" "/workspaces/rust-renderer/tools/trace_replay/build-apitrace/btest")
set_tests_properties([=[libbacktrace_btest]=] PROPERTIES  _BACKTRACE_TRIPLES "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/libbacktrace.cmake;166;add_test;/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/libbacktrace.cmake;0;;/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/CMakeLists.txt;16;include;/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/CMakeLists.txt;87;include_with_scope;/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/CMakeLists.txt;0;")
add_test([=[libbacktrace_stest]=] "/workspaces/rust-renderer/tools/trace_replay/build-apitrace/stest")
set_tests_properties([=[libbacktrace_stest]=] PROPERTIES  _BACKTRACE_TRIPLES "/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/libbacktrace.cmake;181;add_test;/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/libbacktrace.cmake;0;;/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/CMakeLists.txt;16;include;/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/CMakeLists.txt;87;include_with_scope;/workspaces/rust-renderer/tools/trace_replay/vendor/apitrace/thirdparty/CMakeLists.txt;0;")
subdirs("crc32c")
subdirs("md5")
