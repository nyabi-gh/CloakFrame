set(
    CLOAKFRAME_LLVM_TOOL_HINTS
    "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin"
    "/opt/homebrew/opt/llvm@22/bin"
    "/opt/homebrew/opt/llvm/bin"
    "/usr/local/opt/llvm@22/bin"
    "/usr/local/opt/llvm/bin"
)
if(WIN32 AND CMAKE_GENERATOR_INSTANCE)
    list(
        APPEND
        CLOAKFRAME_LLVM_TOOL_HINTS
        "${CMAKE_GENERATOR_INSTANCE}/VC/Tools/Llvm/x64/bin"
        "${CMAKE_GENERATOR_INSTANCE}/VC/Tools/Llvm/bin"
    )
endif()

find_program(
    CLOAKFRAME_CLANG_FORMAT
    NAMES clang-format
    HINTS ${CLOAKFRAME_LLVM_TOOL_HINTS}
)
if(CLOAKFRAME_CLANG_FORMAT)
    file(
        GLOB_RECURSE
        CLOAKFRAME_FORMAT_SOURCES
        CONFIGURE_DEPENDS
        "${CMAKE_CURRENT_SOURCE_DIR}/include/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.mm"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.hpp"
    )
    add_custom_target(
        cloakframe_format
        COMMAND
        "${CLOAKFRAME_CLANG_FORMAT}"
        -i
        ${CLOAKFRAME_FORMAT_SOURCES}
        VERBATIM
    )
    add_custom_target(
        cloakframe_format_check
        COMMAND
        "${CLOAKFRAME_CLANG_FORMAT}"
        --dry-run
        --Werror
        ${CLOAKFRAME_FORMAT_SOURCES}
        VERBATIM
    )
endif()

find_program(
    CLOAKFRAME_CLANG_TIDY
    NAMES clang-tidy
    HINTS ${CLOAKFRAME_LLVM_TOOL_HINTS}
)
find_program(
    CLOAKFRAME_RUN_CLANG_TIDY
    NAMES run-clang-tidy run-clang-tidy.py
    HINTS ${CLOAKFRAME_LLVM_TOOL_HINTS}
)
set(
    CLOAKFRAME_RUN_CLANG_TIDY_COMMAND
    "${CLOAKFRAME_RUN_CLANG_TIDY}"
)
if(WIN32 AND CLOAKFRAME_RUN_CLANG_TIDY)
    find_package(Python3 QUIET COMPONENTS Interpreter)
    if(Python3_Interpreter_FOUND)
        list(
            PREPEND
            CLOAKFRAME_RUN_CLANG_TIDY_COMMAND
            "${Python3_EXECUTABLE}"
        )
    else()
        set(CLOAKFRAME_RUN_CLANG_TIDY_COMMAND)
    endif()
endif()

if(CLOAKFRAME_CLANG_TIDY AND CLOAKFRAME_RUN_CLANG_TIDY_COMMAND)
    set(CLOAKFRAME_TIDY_PATH_SEPARATOR "[/\\\\]")
    string(
        REPLACE
        "/"
        "${CLOAKFRAME_TIDY_PATH_SEPARATOR}"
        CLOAKFRAME_TIDY_SOURCE_ROOT
        "${CMAKE_CURRENT_SOURCE_DIR}"
    )
    set(
        CLOAKFRAME_TIDY_SOURCE_PATTERN
        "^${CLOAKFRAME_TIDY_SOURCE_ROOT}${CLOAKFRAME_TIDY_PATH_SEPARATOR}(src|tests|tools)${CLOAKFRAME_TIDY_PATH_SEPARATOR}.*\\.(cpp|mm)$"
    )
    set(CLOAKFRAME_CLANG_TIDY_ARGUMENTS)
    if(APPLE)
        execute_process(
            COMMAND xcrun --sdk macosx --show-sdk-path
            OUTPUT_VARIABLE CLOAKFRAME_MACOS_SDK_PATH
            OUTPUT_STRIP_TRAILING_WHITESPACE
            RESULT_VARIABLE CLOAKFRAME_MACOS_SDK_RESULT
        )
        if(CLOAKFRAME_MACOS_SDK_RESULT EQUAL 0)
            list(
                APPEND
                CLOAKFRAME_CLANG_TIDY_ARGUMENTS
                -extra-arg=-isysroot
                "-extra-arg=${CLOAKFRAME_MACOS_SDK_PATH}"
            )
        endif()
    endif()

    add_custom_target(
        cloakframe_tidy
        COMMAND
        ${CLOAKFRAME_RUN_CLANG_TIDY_COMMAND}
        -p
        "${CMAKE_BINARY_DIR}"
        -clang-tidy-binary
        "${CLOAKFRAME_CLANG_TIDY}"
        -quiet
        ${CLOAKFRAME_CLANG_TIDY_ARGUMENTS}
        "${CLOAKFRAME_TIDY_SOURCE_PATTERN}"
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        USES_TERMINAL
        VERBATIM
    )

    foreach(
        target
        CloakFrame
        cloakframe_inspect_model
        cloakframe_tests
        cloakframe_tracking_tests
        cloakframe_parallel_tests
        cloakframe_video_io_tests
        cloakframe_video_review_tests
        cloakframe_results_tests
    )
        if(TARGET ${target})
            add_dependencies(cloakframe_tidy ${target})
        endif()
    endforeach()
endif()
