function(add_cloakframe_test name)
    add_executable(${name} ${ARGN})
    if(MSVC)
        target_compile_options(${name} PRIVATE /UNDEBUG)
    else()
        target_compile_options(${name} PRIVATE -UNDEBUG)
    endif()
    target_link_libraries(${name} PRIVATE cloakframe_core)
    cloakframe_harden(${name})
    add_test(NAME ${name} COMMAND ${name})
endfunction()

add_cloakframe_test(cloakframe_tests test_core.cpp)
add_cloakframe_test(cloakframe_custom_model_consent_tests test_custom_model_consent.cpp)
add_cloakframe_test(cloakframe_model_store_tests test_model_store.cpp)
add_cloakframe_test(cloakframe_tracking_tests test_tracking.cpp)
add_cloakframe_test(cloakframe_parallel_tests test_parallel.cpp)
add_cloakframe_test(cloakframe_stage_cleanup_tests test_stage_cleanup.cpp)
add_cloakframe_test(cloakframe_update_cache_tests test_update_cache.cpp)
add_cloakframe_test(cloakframe_update_signature_tests test_update_signature.cpp)
add_cloakframe_test(cloakframe_video_io_tests test_video_io.cpp)
set_tests_properties(cloakframe_video_io_tests PROPERTIES SKIP_RETURN_CODE 77)
add_cloakframe_test(cloakframe_video_review_tests
    test_video_review.cpp
    ${PROJECT_SOURCE_DIR}/src/VideoReviewDialog.cpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/VideoReviewDialog.hpp)
target_link_libraries(cloakframe_video_review_tests PRIVATE Qt6::Widgets)
add_cloakframe_test(cloakframe_results_tests
    test_results.cpp
    ${PROJECT_SOURCE_DIR}/src/ResultsDialog.cpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ResultsDialog.hpp)
target_link_libraries(cloakframe_results_tests PRIVATE Qt6::Widgets)
add_cloakframe_test(cloakframe_model_download_tests test_model_download.cpp)
add_cloakframe_test(cloakframe_logging_tests test_logging.cpp)
add_cloakframe_test(cloakframe_thumbnail_tests test_thumbnails.cpp
    ${PROJECT_SOURCE_DIR}/src/ThumbnailLoader.cpp)
target_link_libraries(cloakframe_thumbnail_tests PRIVATE Qt6::Widgets)
add_cloakframe_test(cloakframe_worker_video_review_tests test_worker_video_review.cpp)
set_tests_properties(cloakframe_worker_video_review_tests PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 90)

add_cloakframe_test(cloakframe_settings_tests test_settings.cpp
    ${PROJECT_SOURCE_DIR}/src/SettingsDialog.cpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/SettingsDialog.hpp)
target_link_libraries(cloakframe_settings_tests PRIVATE Qt6::Widgets)
