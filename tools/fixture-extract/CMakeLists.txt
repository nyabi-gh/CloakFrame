# Added to an exported v1.12.1 tree by run.sh; links the legacy core library.
add_executable(cloakframe_fixture_detect detect.cpp)
target_link_libraries(cloakframe_fixture_detect PRIVATE cloakframe_core)

add_executable(cloakframe_fixture_defects defects.cpp)
target_link_libraries(cloakframe_fixture_defects PRIVATE cloakframe_core)

add_executable(cloakframe_baseline_video baseline_video.cpp)
target_link_libraries(cloakframe_baseline_video PRIVATE cloakframe_core)
