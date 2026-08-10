# libsodium supplies the Ed25519 verification behind UpdateSignature. It ships no CMake
# package config, so pkg-config is the primary route (Homebrew, apt) with a plain path search
# as the fallback for prebuilt Windows archives, which is how the other Windows dependencies
# are already installed in CI.

if(NOT TARGET cloakframe::sodium)
    find_package(PkgConfig QUIET)
    if(PkgConfig_FOUND)
        pkg_check_modules(CLOAKFRAME_SODIUM QUIET IMPORTED_TARGET libsodium)
    endif()

    if(TARGET PkgConfig::CLOAKFRAME_SODIUM)
        add_library(cloakframe::sodium ALIAS PkgConfig::CLOAKFRAME_SODIUM)
        message(STATUS "CloakFrame: libsodium ${CLOAKFRAME_SODIUM_VERSION} (pkg-config)")
    else()
        find_path(CLOAKFRAME_SODIUM_INCLUDE_DIR sodium.h)
        find_library(CLOAKFRAME_SODIUM_LIBRARY NAMES sodium libsodium)
        if(NOT CLOAKFRAME_SODIUM_INCLUDE_DIR OR NOT CLOAKFRAME_SODIUM_LIBRARY)
            message(FATAL_ERROR
                "libsodium not found. It verifies the signature on in-app updates, so a build "
                "without it could not tell a genuine update from a forged one. Install it "
                "(brew install libsodium, apt install libsodium-dev, vcpkg install libsodium) "
                "or point CMAKE_PREFIX_PATH at an existing copy.")
        endif()
        add_library(cloakframe::sodium UNKNOWN IMPORTED GLOBAL)
        set_target_properties(cloakframe::sodium PROPERTIES
            IMPORTED_LOCATION "${CLOAKFRAME_SODIUM_LIBRARY}"
            INTERFACE_INCLUDE_DIRECTORIES "${CLOAKFRAME_SODIUM_INCLUDE_DIR}"
        )
        if(WIN32)
            # The Windows archives ship a static build whose headers need this to resolve.
            set_property(TARGET cloakframe::sodium APPEND PROPERTY
                INTERFACE_COMPILE_DEFINITIONS SODIUM_STATIC)
        endif()
        message(STATUS "CloakFrame: libsodium ${CLOAKFRAME_SODIUM_LIBRARY}")
    endif()
endif()
