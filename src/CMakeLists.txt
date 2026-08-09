set(CLOAKFRAME_APP_OPTIONS)
set(CLOAKFRAME_APP_RESOURCES)
if(APPLE)
    list(APPEND CLOAKFRAME_APP_OPTIONS MACOSX_BUNDLE)
    set(APP_ICON_MACOS "${PROJECT_SOURCE_DIR}/assets/CloakFrame.icns")
    if(EXISTS "${APP_ICON_MACOS}")
        set_source_files_properties("${APP_ICON_MACOS}" PROPERTIES
            MACOSX_PACKAGE_LOCATION "Resources")
        list(APPEND CLOAKFRAME_APP_RESOURCES "${APP_ICON_MACOS}")
    endif()
elseif(WIN32)
    list(APPEND CLOAKFRAME_APP_OPTIONS WIN32)
endif()

add_library(cloakframe_core STATIC
    ImageIo.cpp
    ImageScanner.cpp
    MemoryBudget.cpp
    ModelCatalog.cpp
    Mosaic.cpp
    OnnxGraphPatch.cpp
    OrtAcceleration.cpp
    OutputPlan.cpp
    PlateDetector.cpp
    ProcessorWorker.cpp
    ReleaseNotes.cpp
    SceneCut.cpp
    ScrfdFaceDetector.cpp
    Tracking.cpp
    VideoIo.cpp
    VideoProcessor.cpp
    VideoReviewTypes.cpp
    Yolo5FaceDetector.cpp
    YuNetFaceDetector.cpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/DetectionGeometry.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/Detector.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/FaceDetection.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ImageIo.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ImageScanner.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/MemoryBudget.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ModelCatalog.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/Mosaic.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/OnnxGraphPatch.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/OrderedParallel.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/OrtAcceleration.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/OutputPlan.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/PathSafety.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/PathUtil.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/PlateDetector.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ProcessorWorker.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ReleaseNotes.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ReviewTypes.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/SceneCut.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ScrfdFaceDetector.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/Tracking.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/VideoIo.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/VideoProcessor.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/VideoReviewTypes.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/Yolo5FaceDetector.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/YuNetFaceDetector.hpp
)

target_include_directories(cloakframe_core PUBLIC ${PROJECT_SOURCE_DIR}/include)
target_link_libraries(cloakframe_core PUBLIC
    Qt6::Core
    Qt6::Gui
    Qt6::Network
    ${OpenCV_LIBS}
    onnxruntime::onnxruntime
    spdlog::spdlog
)

if(WIN32)
    target_link_libraries(cloakframe_core PUBLIC advapi32)
endif()

if(CLOAKFRAME_EXIV2_TARGET)
    target_link_libraries(cloakframe_core PUBLIC ${CLOAKFRAME_EXIV2_TARGET})
    target_compile_definitions(cloakframe_core PUBLIC CLOAKFRAME_HAVE_EXIV2)
    message(STATUS "CloakFrame: metadata preservation enabled (${CLOAKFRAME_EXIV2_TARGET})")
else()
    message(WARNING "exiv2 not found; building without metadata preservation.")
endif()

qt_add_executable(CloakFrame ${CLOAKFRAME_APP_OPTIONS}
    main.cpp
    MainWindow.cpp
    ModelDownloader.cpp
    ReviewDialog.cpp
    SettingsDialog.cpp
    Theme.cpp
    UpdateChecker.cpp
    VideoReviewDialog.cpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/MainWindow.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ModelDownloader.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/ReviewDialog.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/SelfUpdater.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/SettingsDialog.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/Theme.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/UpdateChecker.hpp
    ${PROJECT_SOURCE_DIR}/include/cloakframe/VideoReviewDialog.hpp
    ${CLOAKFRAME_APP_RESOURCES}
)

if(CLOAKFRAME_USE_VELOPACK)
    target_sources(CloakFrame PRIVATE SelfUpdaterVelopack.cpp)
    cloakframe_enable_velopack(CloakFrame)
elseif(CLOAKFRAME_USE_SPARKLE)
    target_sources(CloakFrame PRIVATE SelfUpdaterSparkle.mm)
    cloakframe_enable_sparkle(CloakFrame)
else()
    target_sources(CloakFrame PRIVATE SelfUpdaterNull.cpp)
endif()

if(WIN32)
    target_sources(CloakFrame PRIVATE ${PROJECT_SOURCE_DIR}/assets/CloakFrame.rc)
    if(MSVC)
        # Qt generates a manifest of its own for a Windows app unless the target already
        # lists a .manifest source, and CMake embeds such a source through the linker. The
        # resource script must not embed one as well, or the two RT_MANIFEST entries make
        # the resource compiler fail with CVT1100.
        target_sources(CloakFrame PRIVATE ${PROJECT_SOURCE_DIR}/assets/CloakFrame.manifest)
        set_source_files_properties(${PROJECT_SOURCE_DIR}/assets/CloakFrame.rc PROPERTIES
            COMPILE_DEFINITIONS CLOAKFRAME_MANIFEST_FROM_LINKER)
        # A linker-generated manifest would not carry activeCodePage or longPathAware.
        target_link_options(CloakFrame PRIVATE /MANIFEST:NO)
    endif()
endif()

set_target_properties(CloakFrame PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}"
)

target_compile_definitions(CloakFrame PRIVATE CLOAKFRAME_VERSION="${PROJECT_VERSION}")
target_link_libraries(CloakFrame PRIVATE
    cloakframe_core
    Qt6::Widgets
    Qt6::Network
)

if(TARGET Qt6::Svg)
    target_link_libraries(CloakFrame PRIVATE Qt6::Svg)
    target_compile_definitions(CloakFrame PRIVATE CLOAKFRAME_HAVE_SVG)
    message(STATUS "CloakFrame: SVG settings icon enabled")
else()
    message(STATUS "CloakFrame: Qt Svg not found; using a text glyph for the settings icon")
endif()

if(Qt6LinguistTools_FOUND)
    # cloakframe_core has to be scanned as well: it holds the worker, video and publication
    # messages. A target lupdate does not see has its strings marked obsolete, and lrelease
    # drops obsolete strings, so those messages would appear in English in every language.
    qt_add_translations(CloakFrame
        SOURCE_TARGETS CloakFrame cloakframe_core
        TS_FILES ${PROJECT_SOURCE_DIR}/translations/cloakframe_ko.ts
                 ${PROJECT_SOURCE_DIR}/translations/cloakframe_ja.ts
                 ${PROJECT_SOURCE_DIR}/translations/cloakframe_zh_CN.ts
    )
else()
    message(WARNING "Qt LinguistTools not found; building without embedded translations (English only).")
endif()

cloakframe_harden(cloakframe_core)
cloakframe_harden(CloakFrame)

if(APPLE)
    set_target_properties(CloakFrame PROPERTIES
        MACOSX_BUNDLE_GUI_IDENTIFIER "com.cloakframe.app"
        MACOSX_BUNDLE_BUNDLE_NAME "CloakFrame"
        MACOSX_BUNDLE_SHORT_VERSION_STRING "${PROJECT_VERSION}"
        MACOSX_BUNDLE_BUNDLE_VERSION "${PROJECT_VERSION}"
        MACOSX_BUNDLE_ICON_FILE "CloakFrame.icns"
    )
    if(CLOAKFRAME_USE_SPARKLE)
        set_target_properties(CloakFrame PROPERTIES
            MACOSX_BUNDLE_INFO_PLIST "${PROJECT_SOURCE_DIR}/assets/MacOSXBundleInfo.plist.in")
    endif()
endif()

install(TARGETS CloakFrame
    BUNDLE DESTINATION .
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
)

if(UNIX AND NOT APPLE)
    install(FILES ${PROJECT_SOURCE_DIR}/assets/cloakframe.desktop DESTINATION share/applications)
    install(FILES ${PROJECT_SOURCE_DIR}/assets/cloakframe-512.png
        DESTINATION share/icons/hicolor/512x512/apps RENAME cloakframe.png)
endif()
