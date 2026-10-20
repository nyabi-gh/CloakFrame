# Added to an exported v1.12.1 tree by run_legacy.sh; links the legacy core library.
add_executable(cloakframe_bench_legacy legacy.cpp)
target_link_libraries(cloakframe_bench_legacy PRIVATE cloakframe_core)
