include(FetchContent)

set(CLOAKFRAME_VELOPACK_VERSION "1.2.0")
set(CLOAKFRAME_VELOPACK_SHA256
    "547262ed7a1ab1ff62f580aa53851ede2f1a451ac61b8974eb7bc01117488835")

function(cloakframe_enable_velopack target)
    FetchContent_Declare(velopack_libc
        URL "https://github.com/velopack/velopack/releases/download/${CLOAKFRAME_VELOPACK_VERSION}/velopack_libc_${CLOAKFRAME_VELOPACK_VERSION}.zip"
        URL_HASH SHA256=${CLOAKFRAME_VELOPACK_SHA256}
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
    )
    FetchContent_MakeAvailable(velopack_libc)

    if(WIN32)
        if(NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
            message(FATAL_ERROR "Velopack integration expects a 64-bit Windows build")
        endif()
        # The release carries a per-platform file name, but the import library and the
        # DLL's own export directory both name it velopack_libc.dll, and that is the name
        # the loader resolves. Stage it under that name so everything downstream, including
        # the install step, deploys what the import table asks for.
        set(_velopack_dll "${CMAKE_BINARY_DIR}/velopack/velopack_libc.dll")
        file(COPY_FILE
            "${velopack_libc_SOURCE_DIR}/lib/velopack_libc_win_x64_msvc.dll"
            "${_velopack_dll}"
            ONLY_IF_DIFFERENT)

        # GLOBAL: the packaging module runs in the top-level directory, so a target
        # imported with this function's default (src/) scope would not be visible there.
        add_library(velopack::velopack SHARED IMPORTED GLOBAL)
        set_target_properties(velopack::velopack PROPERTIES
            IMPORTED_LOCATION "${_velopack_dll}"
            IMPORTED_IMPLIB "${velopack_libc_SOURCE_DIR}/lib/velopack_libc_win_x64_msvc.dll.lib"
            INTERFACE_INCLUDE_DIRECTORIES "${velopack_libc_SOURCE_DIR}/include"
        )
        add_custom_command(TARGET ${target} POST_BUILD
            COMMAND "${CMAKE_COMMAND}" -E copy_if_different
                "$<TARGET_FILE:velopack::velopack>"
                "$<TARGET_FILE_DIR:${target}>"
        )
    else()
        if(CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64|arm64")
            set(_velopack_arch "arm64")
        else()
            set(_velopack_arch "x64")
        endif()
        add_library(velopack::velopack STATIC IMPORTED GLOBAL)
        set_target_properties(velopack::velopack PROPERTIES
            IMPORTED_LOCATION "${velopack_libc_SOURCE_DIR}/lib-static/velopack_libc_linux_${_velopack_arch}_gnu.a"
            INTERFACE_INCLUDE_DIRECTORIES "${velopack_libc_SOURCE_DIR}/include"
        )
        set_property(TARGET velopack::velopack PROPERTY
            INTERFACE_LINK_LIBRARIES ${CMAKE_DL_LIBS} pthread m)
    endif()

    target_link_libraries(${target} PRIVATE velopack::velopack)
    target_compile_definitions(${target} PRIVATE CLOAKFRAME_HAVE_VELOPACK)
endfunction()
