include(FetchContent)

set(CLOAKFRAME_SPARKLE_PUBLIC_KEY "" CACHE STRING
    "Base64 Sparkle EdDSA public key that updates must be signed with")

# Sparkle only enforces EdDSA when the bundle pins a public key, so the plist entry exists
# exactly when a key is configured. An empty <string> would disable the check silently.
if(CLOAKFRAME_SPARKLE_PUBLIC_KEY)
    set(CLOAKFRAME_SPARKLE_PUBLIC_KEY_ENTRY
        "<key>SUPublicEDKey</key>\n\t<string>${CLOAKFRAME_SPARKLE_PUBLIC_KEY}</string>")
    message(STATUS "CloakFrame: updates require a Sparkle EdDSA signature")
else()
    set(CLOAKFRAME_SPARKLE_PUBLIC_KEY_ENTRY "")
    message(WARNING
        "CLOAKFRAME_SPARKLE_PUBLIC_KEY is unset; macOS updates are trusted on Apple code "
        "signing alone. See CONTRIBUTING.md to create and configure the update key.")
endif()

set(CLOAKFRAME_SPARKLE_VERSION "2.9.4")
set(CLOAKFRAME_SPARKLE_SHA256
    "ce89daf967db1e1893ed3ebd67575ed82d3902563e3191ca92aaec9164fbdef9")

function(cloakframe_enable_sparkle target)
    FetchContent_Declare(sparkle
        URL "https://github.com/sparkle-project/Sparkle/releases/download/${CLOAKFRAME_SPARKLE_VERSION}/Sparkle-${CLOAKFRAME_SPARKLE_VERSION}.tar.xz"
        URL_HASH SHA256=${CLOAKFRAME_SPARKLE_SHA256}
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
    )
    FetchContent_MakeAvailable(sparkle)

    # FetchContent sets sparkle_SOURCE_DIR in this function's scope only; the packaging
    # module needs it from the top-level directory.
    set(CLOAKFRAME_SPARKLE_DIR "${sparkle_SOURCE_DIR}" CACHE INTERNAL
        "Directory holding the fetched Sparkle.framework")

    target_link_libraries(${target} PRIVATE "${sparkle_SOURCE_DIR}/Sparkle.framework")
    set_property(TARGET ${target} APPEND PROPERTY BUILD_RPATH "${sparkle_SOURCE_DIR}")
    target_compile_definitions(${target} PRIVATE CLOAKFRAME_HAVE_SPARKLE)
endfunction()
