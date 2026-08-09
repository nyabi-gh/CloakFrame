include_guard(GLOBAL)

set(CLOAKFRAME_FFMPEG_DIR "" CACHE PATH
    "Directory containing the FFmpeg binaries to bundle")
set(CLOAKFRAME_DIRECTML_DLL "" CACHE FILEPATH
    "DirectML runtime DLL to bundle with a Windows release")
option(CLOAKFRAME_APPIMAGE_LAYOUT
    "Create the top-level AppDir links required by AppImage" OFF)

if(APPLE)
    set(CLOAKFRAME_DOCUMENTATION_DESTINATION
        "CloakFrame.app/Contents/Resources")
    set(CLOAKFRAME_FFMPEG_DESTINATION
        "CloakFrame.app/Contents/Resources/ffmpeg")
elseif(WIN32)
    set(CLOAKFRAME_DOCUMENTATION_DESTINATION ".")
    set(CLOAKFRAME_FFMPEG_DESTINATION "ffmpeg")
else()
    set(CLOAKFRAME_DOCUMENTATION_DESTINATION
        "${CMAKE_INSTALL_DATADIR}/cloakframe")
    set(CLOAKFRAME_FFMPEG_DESTINATION
        "${CMAKE_INSTALL_BINDIR}/ffmpeg")
endif()

install(FILES
    LICENSE
    README.md
    README.en.md
    README.ja.md
    README.zh.md
    THIRD_PARTY_NOTICES.txt
    DESTINATION "${CLOAKFRAME_DOCUMENTATION_DESTINATION}"
)

if(WIN32 AND CLOAKFRAME_USE_VELOPACK)
    install(FILES "$<TARGET_FILE:velopack::velopack>"
        DESTINATION "${CMAKE_INSTALL_BINDIR}")
endif()

if(WIN32 AND CLOAKFRAME_DIRECTML_DLL)
    if(NOT EXISTS "${CLOAKFRAME_DIRECTML_DLL}")
        message(FATAL_ERROR
            "CLOAKFRAME_DIRECTML_DLL does not exist: ${CLOAKFRAME_DIRECTML_DLL}")
    endif()
    install(FILES "${CLOAKFRAME_DIRECTML_DLL}"
        DESTINATION "${CMAKE_INSTALL_BINDIR}")
endif()

if(WIN32)
    # windeployqt deploys Qt modules and plugins only, so every third-party runtime the
    # executable loads has to be installed here or the package cannot start.
    install(FILES "$<TARGET_FILE:onnxruntime::onnxruntime>"
        DESTINATION "${CMAKE_INSTALL_BINDIR}")

    set(_cloakframe_opencv_targets ${OpenCV_LIBS})
    list(REMOVE_DUPLICATES _cloakframe_opencv_targets)
    foreach(_cloakframe_opencv_target IN LISTS _cloakframe_opencv_targets)
        if(NOT TARGET ${_cloakframe_opencv_target})
            continue()
        endif()
        get_target_property(_cloakframe_opencv_type
            ${_cloakframe_opencv_target} TYPE)
        if(NOT _cloakframe_opencv_type MATCHES "STATIC_LIBRARY|INTERFACE_LIBRARY")
            install(FILES "$<TARGET_FILE:${_cloakframe_opencv_target}>"
                DESTINATION "${CMAKE_INSTALL_BINDIR}")
        endif()
    endforeach()
endif()

if(CLOAKFRAME_FFMPEG_DIR)
    if(WIN32)
        set(_cloakframe_ffmpeg_suffix ".exe")
    else()
        set(_cloakframe_ffmpeg_suffix "")
    endif()

    file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/package-checksums")
    foreach(_cloakframe_tool IN ITEMS ffmpeg ffprobe)
        set(_cloakframe_tool_file
            "${CLOAKFRAME_FFMPEG_DIR}/${_cloakframe_tool}${_cloakframe_ffmpeg_suffix}")
        if(NOT EXISTS "${_cloakframe_tool_file}")
            message(FATAL_ERROR
                "Missing bundled tool: ${_cloakframe_tool_file}")
        endif()

        file(SHA256 "${_cloakframe_tool_file}" _cloakframe_tool_sha256)
        set(_cloakframe_checksum_file
            "${CMAKE_CURRENT_BINARY_DIR}/package-checksums/${_cloakframe_tool}${_cloakframe_ffmpeg_suffix}.sha256")
        file(GENERATE
            OUTPUT "${_cloakframe_checksum_file}"
            CONTENT "${_cloakframe_tool_sha256}")
        install(PROGRAMS "${_cloakframe_tool_file}"
            DESTINATION "${CLOAKFRAME_FFMPEG_DESTINATION}")
        install(FILES "${_cloakframe_checksum_file}"
            DESTINATION "${CLOAKFRAME_FFMPEG_DESTINATION}")
    endforeach()
endif()

if(APPLE AND CLOAKFRAME_USE_SPARKLE)
    if(NOT CLOAKFRAME_SPARKLE_DIR)
        message(FATAL_ERROR "Sparkle is enabled but CLOAKFRAME_SPARKLE_DIR is unset")
    endif()
    install(DIRECTORY "${CLOAKFRAME_SPARKLE_DIR}/Sparkle.framework"
        DESTINATION "CloakFrame.app/Contents/Frameworks"
        USE_SOURCE_PERMISSIONS)
endif()

set(_cloakframe_deploy_tool_options)
if(APPLE)
    get_filename_component(_cloakframe_qt_prefix
        "${Qt6_DIR}/../../.." ABSOLUTE)
    list(APPEND _cloakframe_deploy_tool_options
        "-libpath=${_cloakframe_qt_prefix}/lib"
        "-codesign=-")
    if(CLOAKFRAME_USE_SPARKLE)
        list(APPEND _cloakframe_deploy_tool_options
            "-libpath=${CLOAKFRAME_SPARKLE_DIR}")
    endif()
endif()

qt_generate_deploy_app_script(
    TARGET CloakFrame
    OUTPUT_SCRIPT cloakframe_deploy_script
    DEPLOY_TOOL_OPTIONS ${_cloakframe_deploy_tool_options}
    NO_UNSUPPORTED_PLATFORM_ERROR
)
install(SCRIPT ${cloakframe_deploy_script})

if(UNIX AND NOT APPLE AND CLOAKFRAME_APPIMAGE_LAYOUT)
    install(CODE [[
        if("$ENV{DESTDIR}" STREQUAL "")
            message(FATAL_ERROR
                "CLOAKFRAME_APPIMAGE_LAYOUT requires a DESTDIR staging directory")
        endif()
        file(CREATE_LINK "usr/bin/CloakFrame"
            "$ENV{DESTDIR}/AppRun" SYMBOLIC)
        file(CREATE_LINK "usr/share/applications/cloakframe.desktop"
            "$ENV{DESTDIR}/cloakframe.desktop" SYMBOLIC)
        file(CREATE_LINK "usr/share/icons/hicolor/512x512/apps/cloakframe.png"
            "$ENV{DESTDIR}/cloakframe.png" SYMBOLIC)
    ]])
endif()
