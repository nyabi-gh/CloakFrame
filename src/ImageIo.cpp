#include "cloakframe/ImageIo.hpp"

#include "cloakframe/PathUtil.hpp"
#include "cloakframe/StageCleanup.hpp"

#include <QDir>
#include <QImageIOHandler>
#include <QImageReader>

#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>

#include <algorithm>
#include <array>
#include <atomic>
#include <cctype>
#include <cerrno>
#include <climits>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iterator>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <random>
#include <string_view>
#include <system_error>
#include <vector>

#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <Windows.h>
#include <aclapi.h>
#include <fcntl.h>
#include <io.h>
#include <sddl.h>
#include <sys/stat.h>
#else
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

#ifdef __APPLE__
#include <sys/stdio.h>
#endif

#ifdef __linux__
#include <sys/syscall.h>
#ifndef RENAME_NOREPLACE
#define RENAME_NOREPLACE (1U << 0)
#endif
#endif

#ifdef CLOAKFRAME_HAVE_EXIV2
#include <exiv2/exiv2.hpp>
#endif

namespace cloakframe
{
    namespace
    {
        constexpr std::size_t kMaxContainerChunks = 65'536;

#ifdef CLOAKFRAME_HAVE_EXIV2
        std::mutex g_exiv2Mutex;
        std::mutex g_exiv2NamespaceMutex;
        std::once_flag g_exiv2Initialization;
        bool g_exiv2Available = false;

        void exiv2Lock(void *data, const bool lock)
        {
            auto *mutex = static_cast<std::mutex *>(data);
            if (lock)
            {
                mutex->lock();
            }
            else
            {
                mutex->unlock();
            }
        }

        void terminateExiv2()
        {
            const std::lock_guard<std::mutex> lock(g_exiv2Mutex);
            Exiv2::XmpParser::terminate();
        }

        bool initializeExiv2()
        {
            std::call_once(g_exiv2Initialization,
                []
                {
                    g_exiv2Available =
                        Exiv2::XmpParser::initialize(exiv2Lock, &g_exiv2NamespaceMutex);
                    if (g_exiv2Available)
                    {
                        std::atexit(terminateExiv2);
                    }
                });
            return g_exiv2Available;
        }
#endif

        std::string toUtf8(const std::filesystem::path &value)
        {
            const auto utf8 = value.u8string();
            return std::string(utf8.begin(), utf8.end());
        }

        int orientationFromQtTransformation(const QImageIOHandler::Transformations transformation)
        {
            switch (transformation.toInt())
            {
            case QImageIOHandler::TransformationMirror:
                return 2;
            case QImageIOHandler::TransformationRotate180:
                return 3;
            case QImageIOHandler::TransformationFlip:
                return 4;
            case QImageIOHandler::TransformationFlipAndRotate90:
                return 5;
            case QImageIOHandler::TransformationRotate90:
                return 6;
            case QImageIOHandler::TransformationMirrorAndRotate90:
                return 7;
            case QImageIOHandler::TransformationRotate270:
                return 8;
            default:
                return 1;
            }
        }

        int qtImageOrientation(const std::filesystem::path &source)
        {
            QImageReader reader(pathToQString(source));
            reader.setAutoTransform(false);
            return orientationFromQtTransformation(reader.transformation());
        }

        std::uint16_t read16(const unsigned char *bytes, const bool littleEndian)
        {
            if (littleEndian)
            {
                return static_cast<std::uint16_t>(bytes[0])
                       | (static_cast<std::uint16_t>(bytes[1]) << 8U);
            }
            return (static_cast<std::uint16_t>(bytes[0]) << 8U)
                   | static_cast<std::uint16_t>(bytes[1]);
        }

        std::uint32_t read32(const unsigned char *bytes, const bool littleEndian)
        {
            if (littleEndian)
            {
                return static_cast<std::uint32_t>(bytes[0])
                       | (static_cast<std::uint32_t>(bytes[1]) << 8U)
                       | (static_cast<std::uint32_t>(bytes[2]) << 16U)
                       | (static_cast<std::uint32_t>(bytes[3]) << 24U);
            }
            return (static_cast<std::uint32_t>(bytes[0]) << 24U)
                   | (static_cast<std::uint32_t>(bytes[1]) << 16U)
                   | (static_cast<std::uint32_t>(bytes[2]) << 8U)
                   | static_cast<std::uint32_t>(bytes[3]);
        }

        std::uint64_t read64(const unsigned char *bytes, const bool littleEndian)
        {
            std::uint64_t value = 0;
            if (littleEndian)
            {
                for (int index = 7; index >= 0; --index)
                {
                    value = (value << 8U) | bytes[index];
                }
            }
            else
            {
                for (int index = 0; index < 8; ++index)
                {
                    value = (value << 8U) | bytes[index];
                }
            }
            return value;
        }

        std::size_t classicTiffFrameLowerBound(const std::filesystem::path &source)
        {
            std::ifstream input(source, std::ios::binary);
            std::array<unsigned char, 16> header{};
            if (!input.read(reinterpret_cast<char *>(header.data()), header.size()))
            {
                return 0;
            }

            const bool littleEndian = header[0] == 'I' && header[1] == 'I';
            const bool bigEndian = header[0] == 'M' && header[1] == 'M';
            if (!littleEndian && !bigEndian)
            {
                return 0;
            }

            const auto magic = read16(header.data() + 2, littleEndian);
            const bool bigTiff = magic == 43 && read16(header.data() + 4, littleEndian) == 8
                                 && read16(header.data() + 6, littleEndian) == 0;
            if (magic != 42 && !bigTiff)
            {
                return 0;
            }

            std::uint64_t directoryOffset = bigTiff ? read64(header.data() + 8, littleEndian)
                                                    : read32(header.data() + 4, littleEndian);
            std::size_t count = 0;
            while (directoryOffset != 0 && count < 2)
            {
                if (directoryOffset
                    > static_cast<std::uint64_t>(std::numeric_limits<std::streamoff>::max()))
                {
                    return count;
                }
                input.clear();
                input.seekg(static_cast<std::streamoff>(directoryOffset), std::ios::beg);
                std::array<unsigned char, 8> entryCountBytes{};
                const std::size_t countBytes = bigTiff ? 8 : 2;
                if (!input.read(reinterpret_cast<char *>(entryCountBytes.data()),
                        static_cast<std::streamsize>(countBytes)))
                {
                    return count;
                }
                const std::uint64_t entryCount = bigTiff
                                                     ? read64(entryCountBytes.data(), littleEndian)
                                                     : read16(entryCountBytes.data(), littleEndian);
                const std::uint64_t entryBytes = bigTiff ? 20 : 12;
                const auto maximum = std::numeric_limits<std::uint64_t>::max();
                if (directoryOffset > maximum - countBytes
                    || entryCount > (maximum - directoryOffset - countBytes) / entryBytes)
                {
                    return count;
                }
                const std::uint64_t nextOffsetPosition =
                    directoryOffset + countBytes + entryCount * entryBytes;
                if (nextOffsetPosition
                    > static_cast<std::uint64_t>(std::numeric_limits<std::streamoff>::max()))
                {
                    return count;
                }

                input.seekg(static_cast<std::streamoff>(nextOffsetPosition), std::ios::beg);
                std::array<unsigned char, 8> nextOffsetBytes{};
                const std::size_t offsetBytes = bigTiff ? 8 : 4;
                if (!input.read(reinterpret_cast<char *>(nextOffsetBytes.data()),
                        static_cast<std::streamsize>(offsetBytes)))
                {
                    return count;
                }
                ++count;
                directoryOffset = bigTiff ? read64(nextOffsetBytes.data(), littleEndian)
                                          : read32(nextOffsetBytes.data(), littleEndian);
            }
            return count;
        }

        std::size_t animatedContainerFrameLowerBound(const std::filesystem::path &source)
        {
            std::ifstream input(source, std::ios::binary);
            std::array<unsigned char, 12> header{};
            if (!input.read(reinterpret_cast<char *>(header.data()), header.size()))
            {
                return 0;
            }

            const std::array<unsigned char, 8> pngSignature = {
                0x89,
                'P',
                'N',
                'G',
                0x0D,
                0x0A,
                0x1A,
                0x0A,
            };
            if (std::equal(pngSignature.begin(), pngSignature.end(), header.begin()))
            {
                input.seekg(8, std::ios::beg);
                for (std::size_t chunk = 0; chunk < kMaxContainerChunks && input; ++chunk)
                {
                    std::array<unsigned char, 8> chunkHeader{};
                    if (!input.read(
                            reinterpret_cast<char *>(chunkHeader.data()), chunkHeader.size()))
                    {
                        break;
                    }
                    const std::uint32_t length = read32(chunkHeader.data(), false);
                    const bool animationControl = chunkHeader[4] == 'a' && chunkHeader[5] == 'c'
                                                  && chunkHeader[6] == 'T' && chunkHeader[7] == 'L';
                    if (animationControl && length >= 4)
                    {
                        std::array<unsigned char, 4> frameCount{};
                        if (input.read(
                                reinterpret_cast<char *>(frameCount.data()), frameCount.size()))
                        {
                            return read32(frameCount.data(), false) > 1 ? 2 : 1;
                        }
                        return 0;
                    }
                    const bool imageEnd = chunkHeader[4] == 'I' && chunkHeader[5] == 'E'
                                          && chunkHeader[6] == 'N' && chunkHeader[7] == 'D';
                    const bool imageData = chunkHeader[4] == 'I' && chunkHeader[5] == 'D'
                                           && chunkHeader[6] == 'A' && chunkHeader[7] == 'T';
                    input.seekg(static_cast<std::streamoff>(length) + 4, std::ios::cur);
                    if (imageEnd || imageData)
                    {
                        break;
                    }
                }
                return 1;
            }

            const bool webp = header[0] == 'R' && header[1] == 'I' && header[2] == 'F'
                              && header[3] == 'F' && header[8] == 'W' && header[9] == 'E'
                              && header[10] == 'B' && header[11] == 'P';
            if (!webp)
            {
                return 0;
            }

            for (std::size_t chunk = 0; chunk < kMaxContainerChunks && input; ++chunk)
            {
                std::array<unsigned char, 8> chunkHeader{};
                if (!input.read(reinterpret_cast<char *>(chunkHeader.data()), chunkHeader.size()))
                {
                    break;
                }
                const std::uint32_t length = read32(chunkHeader.data() + 4, true);
                const bool animationFrame = chunkHeader[0] == 'A' && chunkHeader[1] == 'N'
                                            && chunkHeader[2] == 'M' && chunkHeader[3] == 'F';
                if (animationFrame)
                {
                    return 2;
                }
                if (chunkHeader[0] == 'V' && chunkHeader[1] == 'P' && chunkHeader[2] == '8'
                    && chunkHeader[3] == 'X' && length > 0)
                {
                    unsigned char flags = 0;
                    if (!input.read(reinterpret_cast<char *>(&flags), 1))
                    {
                        return 0;
                    }
                    if ((flags & 0x02U) != 0)
                    {
                        return 2;
                    }
                    input.seekg(static_cast<std::streamoff>(length) - 1
                                    + static_cast<std::streamoff>(length & 1U),
                        std::ios::cur);
                }
                else
                {
                    input.seekg(static_cast<std::streamoff>(length)
                                    + static_cast<std::streamoff>(length & 1U),
                        std::ios::cur);
                }

                const bool imageData = chunkHeader[0] == 'V' && chunkHeader[1] == 'P'
                                       && chunkHeader[2] == '8'
                                       && (chunkHeader[3] == ' ' || chunkHeader[3] == 'L');
                if (imageData)
                {
                    break;
                }
            }
            return 1;
        }

        std::filesystem::path uniqueTemporaryName()
        {
            static std::atomic<std::uint64_t> sequence{0};
            static thread_local std::mt19937_64 rng{std::random_device{}()};

            return ".cloakframe-" + std::to_string(rng()) + "-"
                   + std::to_string(sequence.fetch_add(1, std::memory_order_relaxed)) + ".tmp";
        }

        bool validRelativeDestination(const std::filesystem::path &relative)
        {
            if (relative.empty() || relative.is_absolute() || relative.has_root_name()
                || relative.filename().empty())
            {
                return false;
            }
            for (const auto &component : relative)
            {
                if (component.empty() || component == "." || component == "..")
                {
                    return false;
                }
            }
            return true;
        }

        int openExclusive(const std::filesystem::path &path)
        {
#ifdef _WIN32
            return ::_wopen(
                path.c_str(), _O_BINARY | _O_CREAT | _O_EXCL | _O_WRONLY, _S_IREAD | _S_IWRITE);
#else
            int flags = O_CREAT | O_EXCL | O_WRONLY;
#ifdef O_CLOEXEC
            flags |= O_CLOEXEC;
#endif
#ifdef O_NOFOLLOW
            flags |= O_NOFOLLOW;
#endif
            return ::open(path.c_str(), flags, 0600);
#endif
        }

        bool writeAll(const int descriptor, const unsigned char *data, std::size_t size)
        {
            while (size > 0)
            {
#ifdef _WIN32
                const auto chunk = static_cast<unsigned int>(
                    std::min<std::size_t>(size, static_cast<std::size_t>(INT_MAX)));
                const int written = ::_write(descriptor, data, chunk);
#else
                const ssize_t written = ::write(descriptor, data, size);
#endif
                if (written < 0)
                {
                    if (errno == EINTR)
                    {
                        continue;
                    }
                    return false;
                }
                if (written == 0)
                {
                    return false;
                }
                data += written;
                size -= static_cast<std::size_t>(written);
            }
            return true;
        }

        bool writeStream(const int descriptor,
            std::istream &input,
            const std::function<bool()> &continueGuard = {})
        {
            constexpr std::size_t guardInterval = std::size_t{16} * 1024 * 1024;
            if (continueGuard && !continueGuard())
            {
                return false;
            }

            std::array<unsigned char, std::size_t{64} * 1024> buffer{};
            std::size_t bytesSinceGuard = 0;
            for (;;)
            {
                input.read(reinterpret_cast<char *>(buffer.data()),
                    static_cast<std::streamsize>(buffer.size()));
                const auto count = input.gcount();
                if (count > 0
                    && !writeAll(descriptor, buffer.data(), static_cast<std::size_t>(count)))
                {
                    return false;
                }
                if (count > 0)
                {
                    bytesSinceGuard += static_cast<std::size_t>(count);
                }
                if (bytesSinceGuard >= guardInterval)
                {
                    if (continueGuard && !continueGuard())
                    {
                        return false;
                    }
                    bytesSinceGuard = 0;
                }
                if (input.eof())
                {
                    return true;
                }
                if (!input)
                {
                    return false;
                }
            }
        }

        bool syncDescriptor(const int descriptor)
        {
#ifdef _WIN32
            return ::_commit(descriptor) == 0;
#else
            return ::fsync(descriptor) == 0;
#endif
        }

        void closeIgnoringErrors(const int descriptor)
        {
#ifdef _WIN32
            ::_close(descriptor);
#else
            ::close(descriptor);
#endif
        }

        class FileDescriptor
        {
        public:
            explicit FileDescriptor(const int descriptor = -1)
                : descriptor_(descriptor)
            {
            }

            ~FileDescriptor()
            {
                if (descriptor_ >= 0)
                {
                    closeIgnoringErrors(descriptor_);
                }
            }

            FileDescriptor(const FileDescriptor &) = delete;
            FileDescriptor &operator=(const FileDescriptor &) = delete;

            [[nodiscard]] int get() const
            {
                return descriptor_;
            }

            [[nodiscard]] bool valid() const
            {
                return descriptor_ >= 0;
            }

        private:
            int descriptor_ = -1;
        };

        std::optional<FileIdentity> descriptorIdentity(const int descriptor)
        {
#ifdef _WIN32
            const intptr_t rawHandle = ::_get_osfhandle(descriptor);
            if (rawHandle == -1)
            {
                return std::nullopt;
            }
            BY_HANDLE_FILE_INFORMATION info{};
            FILE_BASIC_INFO basic{};
            const HANDLE handle = reinterpret_cast<HANDLE>(rawHandle);
            if (!::GetFileInformationByHandle(handle, &info)
                || !::GetFileInformationByHandleEx(handle, FileBasicInfo, &basic, sizeof(basic))
                || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0)
            {
                return std::nullopt;
            }
            FileIdentity identity;
            identity.device = info.dwVolumeSerialNumber;
            identity.file =
                (static_cast<std::uint64_t>(info.nFileIndexHigh) << 32U) | info.nFileIndexLow;
            identity.size =
                (static_cast<std::uint64_t>(info.nFileSizeHigh) << 32U) | info.nFileSizeLow;
            identity.modifiedSeconds = static_cast<std::int64_t>(
                (static_cast<std::uint64_t>(info.ftLastWriteTime.dwHighDateTime) << 32U)
                | info.ftLastWriteTime.dwLowDateTime);
            identity.changedSeconds = basic.ChangeTime.QuadPart;
            return identity;
#else
            struct stat info{};
            if (::fstat(descriptor, &info) != 0 || !S_ISREG(info.st_mode) || info.st_size < 0)
            {
                return std::nullopt;
            }
            FileIdentity identity;
            identity.device = static_cast<std::uint64_t>(info.st_dev);
            identity.file = static_cast<std::uint64_t>(info.st_ino);
            identity.size = static_cast<std::uint64_t>(info.st_size);
#if defined(__APPLE__)
            identity.modifiedSeconds = info.st_mtimespec.tv_sec;
            identity.modifiedNanoseconds = info.st_mtimespec.tv_nsec;
            identity.changedSeconds = info.st_ctimespec.tv_sec;
            identity.changedNanoseconds = info.st_ctimespec.tv_nsec;
#else
            identity.modifiedSeconds = info.st_mtim.tv_sec;
            identity.modifiedNanoseconds = info.st_mtim.tv_nsec;
            identity.changedSeconds = info.st_ctim.tv_sec;
            identity.changedNanoseconds = info.st_ctim.tv_nsec;
#endif
            return identity;
#endif
        }

        FileDescriptor openReadOnlyDescriptor(const std::filesystem::path &path)
        {
#ifdef _WIN32
            const HANDLE handle = ::CreateFileW(path.c_str(),
                GENERIC_READ | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ,
                nullptr,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
                nullptr);
            if (handle == INVALID_HANDLE_VALUE)
            {
                return FileDescriptor{};
            }
            const int descriptor =
                ::_open_osfhandle(reinterpret_cast<intptr_t>(handle), _O_BINARY | _O_RDONLY);
            if (descriptor < 0)
            {
                ::CloseHandle(handle);
            }
            return FileDescriptor(descriptor);
#else
            int flags = O_RDONLY;
#ifdef O_CLOEXEC
            flags |= O_CLOEXEC;
#endif
#ifdef O_NOFOLLOW
            flags |= O_NOFOLLOW;
#endif
            return FileDescriptor(::open(path.c_str(), flags));
#endif
        }

        bool copyDescriptor(const int sourceDescriptor,
            const int destinationDescriptor,
            const std::function<bool()> &continueGuard)
        {
            constexpr std::size_t guardInterval = std::size_t{16} * 1024 * 1024;
            std::array<unsigned char, std::size_t{64} * 1024> buffer{};
            std::size_t bytesSinceGuard = 0;
            if (continueGuard && !continueGuard())
            {
                return false;
            }
            for (;;)
            {
#ifdef _WIN32
                const int count = ::_read(
                    sourceDescriptor, buffer.data(), static_cast<unsigned int>(buffer.size()));
#else
                const ssize_t count = ::read(sourceDescriptor, buffer.data(), buffer.size());
#endif
                if (count < 0)
                {
                    if (errno == EINTR)
                    {
                        continue;
                    }
                    return false;
                }
                if (count == 0)
                {
                    return !continueGuard || continueGuard();
                }
                if (!writeAll(
                        destinationDescriptor, buffer.data(), static_cast<std::size_t>(count)))
                {
                    return false;
                }
                bytesSinceGuard += static_cast<std::size_t>(count);
                if (bytesSinceGuard >= guardInterval)
                {
                    if (continueGuard && !continueGuard())
                    {
                        return false;
                    }
                    bytesSinceGuard = 0;
                }
            }
        }

#ifdef _WIN32
        class WindowsHandle
        {
        public:
            WindowsHandle() = default;

            explicit WindowsHandle(HANDLE handle)
                : handle_(handle)
            {
            }

            ~WindowsHandle()
            {
                reset();
            }

            WindowsHandle(const WindowsHandle &) = delete;
            WindowsHandle &operator=(const WindowsHandle &) = delete;

            WindowsHandle(WindowsHandle &&other) noexcept
                : handle_(other.release())
            {
            }

            WindowsHandle &operator=(WindowsHandle &&other) noexcept
            {
                if (this != &other)
                {
                    reset(other.release());
                }
                return *this;
            }

            [[nodiscard]] HANDLE get() const
            {
                return handle_;
            }

            [[nodiscard]] bool valid() const
            {
                return handle_ != INVALID_HANDLE_VALUE && handle_ != nullptr;
            }

            HANDLE release()
            {
                const HANDLE value = handle_;
                handle_ = INVALID_HANDLE_VALUE;
                return value;
            }

            void reset(HANDLE handle = INVALID_HANDLE_VALUE)
            {
                if (valid())
                {
                    ::CloseHandle(handle_);
                }
                handle_ = handle;
            }

        private:
            HANDLE handle_ = INVALID_HANDLE_VALUE;
        };

        struct WindowsParent
        {
            std::vector<WindowsHandle> directories;
            std::filesystem::path path;
            std::wstring filename;
        };

        WindowsHandle openPinnedDirectory(const std::filesystem::path &path)
        {
            WindowsHandle handle(::CreateFileW(path.c_str(),
                FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                nullptr,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                nullptr));
            if (!handle.valid())
            {
                return {};
            }

            BY_HANDLE_FILE_INFORMATION info{};
            if (!::GetFileInformationByHandle(handle.get(), &info)
                || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) == 0
                || (info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0)
            {
                return {};
            }
            return handle;
        }

        std::optional<WindowsParent> openWindowsParent(const std::filesystem::path &outputRoot,
            const std::filesystem::path &relativeDestination)
        {
            if (!validRelativeDestination(relativeDestination))
            {
                return std::nullopt;
            }

            std::error_code ec;
            const auto canonicalRoot = std::filesystem::canonical(outputRoot, ec);
            const auto absoluteRoot = std::filesystem::absolute(outputRoot, ec).lexically_normal();
            if (ec || !canonicalRoot.is_absolute() || canonicalRoot != absoluteRoot)
            {
                return std::nullopt;
            }

            WindowsParent result;
            std::filesystem::path current = canonicalRoot.root_path();
            auto rootHandle = openPinnedDirectory(current);
            if (!rootHandle.valid())
            {
                return std::nullopt;
            }
            result.directories.push_back(std::move(rootHandle));

            for (const auto &component : canonicalRoot.relative_path())
            {
                current /= component;
                auto handle = openPinnedDirectory(current);
                if (!handle.valid())
                {
                    return std::nullopt;
                }
                result.directories.push_back(std::move(handle));
            }

            for (const auto &component : relativeDestination.parent_path())
            {
                current /= component;
                if (!::CreateDirectoryW(current.c_str(), nullptr)
                    && ::GetLastError() != ERROR_ALREADY_EXISTS)
                {
                    return std::nullopt;
                }
                auto handle = openPinnedDirectory(current);
                if (!handle.valid())
                {
                    return std::nullopt;
                }
                result.directories.push_back(std::move(handle));
            }

            result.path = current;
            result.filename = relativeDestination.filename().native();
            return result;
        }

        PSECURITY_DESCRIPTOR privateSecurityDescriptor()
        {
            PSECURITY_DESCRIPTOR descriptor = nullptr;
            if (!::ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    L"D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;OW)",
                    SDDL_REVISION_1,
                    &descriptor,
                    nullptr))
            {
                return nullptr;
            }
            return descriptor;
        }

        template <typename Writer>
        bool writeTemporaryAndPublishAtRoot(const std::filesystem::path &outputRoot,
            const std::filesystem::path &relativeDestination,
            Writer &&writer,
            const std::function<bool()> &publishGuard = {})
        {
            auto parent = openWindowsParent(outputRoot, relativeDestination);
            if (!parent)
            {
                return false;
            }

            PSECURITY_DESCRIPTOR descriptor = privateSecurityDescriptor();
            if (descriptor == nullptr)
            {
                return false;
            }
            SECURITY_ATTRIBUTES attributes{sizeof(SECURITY_ATTRIBUTES), descriptor, FALSE};

            for (int attempt = 0; attempt < 32; ++attempt)
            {
                const auto stageName = uniqueTemporaryName();
                const auto stagePath = parent->path / stageName;
                if (!::CreateDirectoryW(stagePath.c_str(), &attributes))
                {
                    if (::GetLastError() == ERROR_ALREADY_EXISTS)
                    {
                        continue;
                    }
                    ::LocalFree(descriptor);
                    return false;
                }

                auto stageDirectory = openPinnedDirectory(stagePath);
                if (!stageDirectory.valid())
                {
                    ::RemoveDirectoryW(stagePath.c_str());
                    ::LocalFree(descriptor);
                    return false;
                }

                const auto payloadPath = stagePath / L"payload";
                WindowsHandle payload(::CreateFileW(payloadPath.c_str(),
                    GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES,
                    0,
                    &attributes,
                    CREATE_NEW,
                    FILE_ATTRIBUTE_NORMAL | FILE_FLAG_WRITE_THROUGH,
                    nullptr));
                if (!payload.valid())
                {
                    stageDirectory.reset();
                    ::RemoveDirectoryW(stagePath.c_str());
                    ::LocalFree(descriptor);
                    return false;
                }

                const intptr_t rawHandle = reinterpret_cast<intptr_t>(payload.release());
                const int fileDescriptor = ::_open_osfhandle(rawHandle, _O_BINARY | _O_WRONLY);
                if (fileDescriptor < 0)
                {
                    ::CloseHandle(reinterpret_cast<HANDLE>(rawHandle));
                    ::DeleteFileW(payloadPath.c_str());
                    stageDirectory.reset();
                    ::RemoveDirectoryW(stagePath.c_str());
                    ::LocalFree(descriptor);
                    return false;
                }

                bool wrote = false;
                try
                {
                    wrote = writer(fileDescriptor);
                }
                catch (...)
                {
                    ::_close(fileDescriptor);
                    ::DeleteFileW(payloadPath.c_str());
                    stageDirectory.reset();
                    ::RemoveDirectoryW(stagePath.c_str());
                    ::LocalFree(descriptor);
                    throw;
                }

                const bool synced = wrote && syncDescriptor(fileDescriptor);
                const bool canPublish = synced && (!publishGuard || publishGuard());
                const HANDLE publishHandle =
                    reinterpret_cast<HANDLE>(::_get_osfhandle(fileDescriptor));
                bool published = false;
                if (canPublish && publishHandle != INVALID_HANDLE_VALUE)
                {
                    const std::wstring destinationName = (parent->path / parent->filename).native();
                    const std::size_t nameBytes = destinationName.size() * sizeof(wchar_t);
                    if (nameBytes <= std::numeric_limits<DWORD>::max())
                    {
                        const std::size_t infoSize = sizeof(FILE_RENAME_INFO) + nameBytes;
                        const std::size_t storageCount =
                            (infoSize + sizeof(std::max_align_t) - 1) / sizeof(std::max_align_t);
                        std::vector<std::max_align_t> storage(storageCount);
                        std::memset(storage.data(), 0, storage.size() * sizeof(std::max_align_t));
                        auto *renameInfo = reinterpret_cast<FILE_RENAME_INFO *>(storage.data());
                        renameInfo->RootDirectory = nullptr;
                        renameInfo->FileNameLength = static_cast<DWORD>(nameBytes);
                        std::memcpy(renameInfo->FileName, destinationName.data(), nameBytes);
                        published = ::SetFileInformationByHandle(publishHandle,
                                        FileRenameInfo,
                                        renameInfo,
                                        static_cast<DWORD>(infoSize))
                                    != 0;
                    }
                }

                if (!published && publishHandle != INVALID_HANDLE_VALUE)
                {
                    FILE_DISPOSITION_INFO disposition{TRUE};
                    ::SetFileInformationByHandle(
                        publishHandle, FileDispositionInfo, &disposition, sizeof(disposition));
                }
                ::_close(fileDescriptor);
                if (!published)
                {
                    ::DeleteFileW(payloadPath.c_str());
                }
                stageDirectory.reset();
                ::RemoveDirectoryW(stagePath.c_str());
                ::LocalFree(descriptor);
                return published;
            }

            ::LocalFree(descriptor);
            return false;
        }
#else
        class PosixDescriptor
        {
        public:
            PosixDescriptor() = default;

            explicit PosixDescriptor(int descriptor)
                : descriptor_(descriptor)
            {
            }

            ~PosixDescriptor()
            {
                reset();
            }

            PosixDescriptor(const PosixDescriptor &) = delete;
            PosixDescriptor &operator=(const PosixDescriptor &) = delete;

            PosixDescriptor(PosixDescriptor &&other) noexcept
                : descriptor_(other.release())
            {
            }

            PosixDescriptor &operator=(PosixDescriptor &&other) noexcept
            {
                if (this != &other)
                {
                    reset(other.release());
                }
                return *this;
            }

            [[nodiscard]] int get() const
            {
                return descriptor_;
            }

            [[nodiscard]] bool valid() const
            {
                return descriptor_ >= 0;
            }

            int release()
            {
                const int value = descriptor_;
                descriptor_ = -1;
                return value;
            }

            void reset(int descriptor = -1)
            {
                if (valid())
                {
                    ::close(descriptor_);
                }
                descriptor_ = descriptor;
            }

        private:
            int descriptor_ = -1;
        };

        struct PosixParent
        {
            PosixDescriptor descriptor;
            std::string filename;
        };

        int directoryOpenFlags()
        {
            int flags = O_RDONLY | O_DIRECTORY;
#ifdef O_CLOEXEC
            flags |= O_CLOEXEC;
#endif
#ifdef O_NOFOLLOW
            flags |= O_NOFOLLOW;
#endif
            return flags;
        }

        std::optional<PosixParent> openPosixParent(const std::filesystem::path &outputRoot,
            const std::filesystem::path &relativeDestination)
        {
            if (!validRelativeDestination(relativeDestination))
            {
                return std::nullopt;
            }

            std::error_code ec;
            const auto canonicalRoot = std::filesystem::canonical(outputRoot, ec);
            const auto absoluteRoot = std::filesystem::absolute(outputRoot, ec).lexically_normal();
            if (ec || !canonicalRoot.is_absolute() || canonicalRoot != absoluteRoot)
            {
                return std::nullopt;
            }

            PosixDescriptor current(
                ::open(canonicalRoot.root_path().c_str(), directoryOpenFlags()));
            if (!current.valid())
            {
                return std::nullopt;
            }

            for (const auto &component : canonicalRoot.relative_path())
            {
                PosixDescriptor next(
                    ::openat(current.get(), component.c_str(), directoryOpenFlags()));
                if (!next.valid())
                {
                    return std::nullopt;
                }
                current = std::move(next);
            }

            for (const auto &component : relativeDestination.parent_path())
            {
                if (::mkdirat(current.get(), component.c_str(), 0700) != 0 && errno != EEXIST)
                {
                    return std::nullopt;
                }
                PosixDescriptor next(
                    ::openat(current.get(), component.c_str(), directoryOpenFlags()));
                if (!next.valid())
                {
                    return std::nullopt;
                }
                current = std::move(next);
            }

            return PosixParent{std::move(current), relativeDestination.filename().native()};
        }

        // Filesystems without an exclusive rename and without hardlinks (FAT, exFAT, some
        // network and FUSE mounts) cannot publish atomically. The copy fallback goes through
        // this name instead of the destination, so an interrupted publication leaves something
        // obviously incomplete rather than a truncated file under the name a user would read as
        // a finished result.
        constexpr const char *kPartialSuffix = ".cloakframe-partial";

        std::atomic<bool> atomicPublicationDisabled{false};

        bool publishPosixNoReplace(const int stageDescriptor,
            const char *payload,
            const int parentDescriptor,
            const char *destination,
            const std::function<bool()> &publishGuard = {})
        {
            if (!atomicPublicationDisabled.load(std::memory_order_relaxed))
            {
#ifdef __APPLE__
                if (::renameatx_np(
                        stageDescriptor, payload, parentDescriptor, destination, RENAME_EXCL)
                    == 0)
                {
                    return true;
                }
                if (errno != ENOTSUP && errno != EINVAL)
                {
                    return false;
                }
#elif defined(__linux__) && defined(SYS_renameat2)
                if (::syscall(SYS_renameat2,
                        stageDescriptor,
                        payload,
                        parentDescriptor,
                        destination,
                        RENAME_NOREPLACE)
                    == 0)
                {
                    return true;
                }
                if (errno != ENOSYS && errno != EINVAL && errno != ENOTSUP)
                {
                    return false;
                }
#endif
                if (::linkat(stageDescriptor, payload, parentDescriptor, destination, 0) == 0)
                {
                    ::unlinkat(stageDescriptor, payload, 0);
                    return true;
                }
                if (errno == EEXIST)
                {
                    return false;
                }
                if (errno != ENOTSUP && errno != EOPNOTSUPP && errno != EPERM && errno != ENOSYS)
                {
                    return false;
                }
            }
            PosixDescriptor source(
                ::openat(stageDescriptor, payload, O_RDONLY | O_NOFOLLOW | O_CLOEXEC));
            if (!source.valid())
            {
                return false;
            }
            struct stat sourceInfo{};
            if (::fstat(source.get(), &sourceInfo) != 0 || !S_ISREG(sourceInfo.st_mode))
            {
                return false;
            }
            const std::string partial = std::string(destination) + kPartialSuffix;
            PosixDescriptor target(::openat(parentDescriptor,
                partial.c_str(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                sourceInfo.st_mode & 07777));
            if (!target.valid())
            {
                return false;
            }
            const auto abandonTarget = [&]
            {
                ::unlinkat(parentDescriptor, partial.c_str(), 0);
                return false;
            };
            std::array<char, 65536> buffer{};
            // Consulting the guard costs a stat, so it is polled per megabyte rather than per
            // chunk. A cancelled run must not keep copying to the end.
            constexpr int kChunksPerGuardCheck = 16;
            int chunksSinceGuard = 0;
            for (;;)
            {
                if (publishGuard && ++chunksSinceGuard >= kChunksPerGuardCheck)
                {
                    chunksSinceGuard = 0;
                    if (!publishGuard())
                    {
                        return abandonTarget();
                    }
                }
                const ssize_t got = ::read(source.get(), buffer.data(), buffer.size());
                if (got == 0)
                {
                    break;
                }
                if (got < 0)
                {
                    if (errno == EINTR)
                    {
                        continue;
                    }
                    return abandonTarget();
                }
                const char *cursor = buffer.data();
                ssize_t remaining = got;
                while (remaining > 0)
                {
                    const ssize_t wrote =
                        ::write(target.get(), cursor, static_cast<std::size_t>(remaining));
                    if (wrote < 0)
                    {
                        if (errno == EINTR)
                        {
                            continue;
                        }
                        return abandonTarget();
                    }
                    cursor += wrote;
                    remaining -= wrote;
                }
            }
            if (::fsync(target.get()) != 0)
            {
                return abandonTarget();
            }
            target.reset();

            // The destination name is claimed only now, so the window in which another writer
            // could take it is a single rename rather than the whole copy. `renameat` replaces,
            // so the name has to be checked immediately before it.
            struct stat existing{};
            if (::fstatat(parentDescriptor, destination, &existing, AT_SYMLINK_NOFOLLOW) == 0
                || errno != ENOENT)
            {
                return abandonTarget();
            }
            if (::renameat(parentDescriptor, partial.c_str(), parentDescriptor, destination) != 0)
            {
                return abandonTarget();
            }
            ::unlinkat(stageDescriptor, payload, 0);
            return true;
        }

        template <typename Writer>
        bool writeTemporaryAndPublishAtRoot(const std::filesystem::path &outputRoot,
            const std::filesystem::path &relativeDestination,
            Writer &&writer,
            const std::function<bool()> &publishGuard = {})
        {
            auto parent = openPosixParent(outputRoot, relativeDestination);
            if (!parent)
            {
                return false;
            }

            for (int attempt = 0; attempt < 32; ++attempt)
            {
                const auto stageName = uniqueTemporaryName().native();
                if (::mkdirat(parent->descriptor.get(), stageName.c_str(), 0700) != 0)
                {
                    if (errno == EEXIST)
                    {
                        continue;
                    }
                    return false;
                }

                PosixDescriptor stage(
                    ::openat(parent->descriptor.get(), stageName.c_str(), directoryOpenFlags()));
                if (!stage.valid())
                {
                    ::unlinkat(parent->descriptor.get(), stageName.c_str(), AT_REMOVEDIR);
                    return false;
                }

                int fileFlags = O_CREAT | O_EXCL | O_WRONLY;
#ifdef O_CLOEXEC
                fileFlags |= O_CLOEXEC;
#endif
#ifdef O_NOFOLLOW
                fileFlags |= O_NOFOLLOW;
#endif
                PosixDescriptor file(::openat(stage.get(), "payload", fileFlags, 0600));
                if (!file.valid())
                {
                    stage.reset();
                    ::unlinkat(parent->descriptor.get(), stageName.c_str(), AT_REMOVEDIR);
                    return false;
                }

                bool wrote = false;
                try
                {
                    wrote = writer(file.get());
                }
                catch (...)
                {
                    file.reset();
                    ::unlinkat(stage.get(), "payload", 0);
                    stage.reset();
                    ::unlinkat(parent->descriptor.get(), stageName.c_str(), AT_REMOVEDIR);
                    throw;
                }

                if (!wrote || !syncDescriptor(file.get()))
                {
                    file.reset();
                    ::unlinkat(stage.get(), "payload", 0);
                    stage.reset();
                    ::unlinkat(parent->descriptor.get(), stageName.c_str(), AT_REMOVEDIR);
                    return false;
                }

                const bool published = (!publishGuard || publishGuard())
                                       && publishPosixNoReplace(stage.get(),
                                           "payload",
                                           parent->descriptor.get(),
                                           parent->filename.c_str(),
                                           publishGuard);
                file.reset();
                ::unlinkat(stage.get(), "payload", 0);
                stage.reset();
                ::unlinkat(parent->descriptor.get(), stageName.c_str(), AT_REMOVEDIR);
                if (published)
                {
                    ::fsync(parent->descriptor.get());
                }
                return published;
            }
            return false;
        }
#endif
    }

    bool metadataSupportAvailable()
    {
#ifdef CLOAKFRAME_HAVE_EXIV2
        return initializeExiv2();
#else
        return false;
#endif
    }

    void setAtomicPublicationDisabledForTesting([[maybe_unused]] const bool disabled)
    {
#ifndef _WIN32
        atomicPublicationDisabled.store(disabled, std::memory_order_relaxed);
#endif
    }

    std::optional<FileIdentity> captureFileIdentity(const std::filesystem::path &path)
    {
#ifdef _WIN32
        WindowsHandle handle(::CreateFileW(path.c_str(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            nullptr,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            nullptr));
        if (!handle.valid())
        {
            return std::nullopt;
        }

        BY_HANDLE_FILE_INFORMATION info{};
        FILE_BASIC_INFO basic{};
        if (!::GetFileInformationByHandle(handle.get(), &info)
            || !::GetFileInformationByHandleEx(handle.get(), FileBasicInfo, &basic, sizeof(basic))
            || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0)
        {
            return std::nullopt;
        }

        FileIdentity identity;
        identity.device = info.dwVolumeSerialNumber;
        identity.file =
            (static_cast<std::uint64_t>(info.nFileIndexHigh) << 32U) | info.nFileIndexLow;
        identity.size = (static_cast<std::uint64_t>(info.nFileSizeHigh) << 32U) | info.nFileSizeLow;
        identity.modifiedSeconds = static_cast<std::int64_t>(
            (static_cast<std::uint64_t>(info.ftLastWriteTime.dwHighDateTime) << 32U)
            | info.ftLastWriteTime.dwLowDateTime);
        identity.changedSeconds = basic.ChangeTime.QuadPart;
        return identity;
#else
        struct stat info{};
        if (::stat(path.c_str(), &info) != 0 || !S_ISREG(info.st_mode) || info.st_size < 0)
        {
            return std::nullopt;
        }

        FileIdentity identity;
        identity.device = static_cast<std::uint64_t>(info.st_dev);
        identity.file = static_cast<std::uint64_t>(info.st_ino);
        identity.size = static_cast<std::uint64_t>(info.st_size);
#if defined(__APPLE__)
        identity.modifiedSeconds = info.st_mtimespec.tv_sec;
        identity.modifiedNanoseconds = info.st_mtimespec.tv_nsec;
        identity.changedSeconds = info.st_ctimespec.tv_sec;
        identity.changedNanoseconds = info.st_ctimespec.tv_nsec;
#else
        identity.modifiedSeconds = info.st_mtim.tv_sec;
        identity.modifiedNanoseconds = info.st_mtim.tv_nsec;
        identity.changedSeconds = info.st_ctim.tv_sec;
        identity.changedNanoseconds = info.st_ctim.tv_nsec;
#endif
        return identity;
#endif
    }

    int readExifOrientation(const std::filesystem::path &source)
    {
#ifdef CLOAKFRAME_HAVE_EXIV2
        if (!initializeExiv2())
        {
            return qtImageOrientation(source);
        }
        const std::lock_guard<std::mutex> lock(g_exiv2Mutex);
        try
        {
            auto image = Exiv2::ImageFactory::open(toUtf8(source));
            image->readMetadata();
            const Exiv2::ExifData &exif = image->exifData();
            const auto it = exif.findKey(Exiv2::ExifKey("Exif.Image.Orientation"));
            if (it != exif.end())
            {
#if EXIV2_TEST_VERSION(0, 28, 0)
                const auto value = static_cast<int>(it->toInt64());
#else
                const auto value = static_cast<int>(it->toLong());
#endif
                if (value >= 1 && value <= 8)
                {
                    return value;
                }
            }
        }
        // Orientation is advisory: fall through to the Qt reader below.
        // NOLINTNEXTLINE(bugprone-empty-catch)
        catch (const std::exception &)
        {
        }
#endif
        return qtImageOrientation(source);
    }

    void applyOrientation(cv::Mat &image, int orientation)
    {
        switch (orientation)
        {
        case 2:
            cv::flip(image, image, 1);
            break;
        case 3:
            cv::rotate(image, image, cv::ROTATE_180);
            break;
        case 4:
            cv::flip(image, image, 0);
            break;
        case 5:
            cv::transpose(image, image);
            break;
        case 6:
            cv::rotate(image, image, cv::ROTATE_90_CLOCKWISE);
            break;
        case 7:
            cv::transpose(image, image);
            cv::rotate(image, image, cv::ROTATE_180);
            break;
        case 8:
            cv::rotate(image, image, cv::ROTATE_90_COUNTERCLOCKWISE);
            break;
        default:
            break;
        }
    }

    cv::Mat toDetectionBgr(const cv::Mat &image)
    {
        cv::Mat eightBit;
        if (image.depth() == CV_16U)
        {
            image.convertTo(eightBit, CV_8U, 1.0 / 257.0);
        }
        else if (image.depth() != CV_8U)
        {
            double minVal = 0.0;
            double maxVal = 0.0;
            cv::minMaxLoc(image.reshape(1), &minVal, &maxVal);
            const double scale = (maxVal > minVal) ? 255.0 / (maxVal - minVal) : 1.0;
            image.convertTo(eightBit, CV_8U, scale, -minVal * scale);
        }
        else
        {
            eightBit = image;
        }

        cv::Mat bgr;
        if (eightBit.channels() == 1)
        {
            cv::cvtColor(eightBit, bgr, cv::COLOR_GRAY2BGR);
        }
        else if (eightBit.channels() == 4)
        {
            cv::cvtColor(eightBit, bgr, cv::COLOR_BGRA2BGR);
        }
        else
        {
            bgr = eightBit;
        }
        return bgr;
    }

    cv::Mat imreadUnicode(const std::filesystem::path &source, int flags)
    {
        std::ifstream file(source, std::ios::binary);
        if (!file)
        {
            return {};
        }
        std::vector<uchar> buffer(
            (std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
        if (buffer.empty())
        {
            return {};
        }
        return cv::imdecode(buffer, flags);
    }

    std::size_t imageFrameCount(const std::filesystem::path &source)
    {
        std::size_t count =
            std::max(classicTiffFrameLowerBound(source), animatedContainerFrameLowerBound(source));
        if (count > 1)
        {
            return count;
        }

        QImageReader reader(pathToQString(source));
        reader.setAutoTransform(false);
        const int qtCount = reader.imageCount();
        if (qtCount > 0)
        {
            count = static_cast<std::size_t>(qtCount);
        }

        try
        {
            count = std::max(count, cv::imcount(toUtf8(source), cv::IMREAD_UNCHANGED));
        }
        // imcount is a best-effort second opinion; the count read above stands.
        // NOLINTNEXTLINE(bugprone-empty-catch)
        catch (const cv::Exception &)
        {
        }
        return count;
    }

    bool imwriteUnicodeNoReplace(const std::filesystem::path &destination,
        const cv::Mat &image,
        const std::vector<int> &params)
    {
        std::error_code ec;
        const auto absoluteDestination = std::filesystem::absolute(destination, ec);
        if (ec)
        {
            return false;
        }
        const auto outputRoot = std::filesystem::canonical(absoluteDestination.parent_path(), ec);
        if (ec)
        {
            return false;
        }
        return imwriteUnicodeNoReplaceAtRoot(
                   outputRoot, absoluteDestination.filename(), image, params)
               != ImageWriteResult::Failed;
    }

    ImageWriteResult imwriteUnicodeNoReplaceAtRoot(const std::filesystem::path &outputRoot,
        const std::filesystem::path &relativeDestination,
        const cv::Mat &image,
        const std::vector<int> &params,
        const std::filesystem::path &metadataSource,
        const std::function<bool()> &publishGuard)
    {
        if (!validRelativeDestination(relativeDestination))
        {
            return ImageWriteResult::Failed;
        }
        if (publishGuard && !publishGuard())
        {
            return ImageWriteResult::Failed;
        }
        const auto extension = relativeDestination.extension();
        if (extension.empty())
        {
            return ImageWriteResult::Failed;
        }

        std::vector<uchar> buffer;
        if (!cv::imencode(extension.string(), image, buffer, params))
        {
            return ImageWriteResult::Failed;
        }

        if (!metadataSource.empty())
        {
            StageDirectory staging(QDir::tempPath());
            if (staging.isValid())
            {
                const auto stagedPath = pathFromQString(
                    staging.filePath(QStringLiteral("image") + pathToQString(extension)));
                const int stagedDescriptor = openExclusive(stagedPath);
                if (stagedDescriptor >= 0)
                {
                    const bool staged = writeAll(stagedDescriptor, buffer.data(), buffer.size())
                                        && syncDescriptor(stagedDescriptor);
                    closeIgnoringErrors(stagedDescriptor);
                    if (staged && (!publishGuard || publishGuard())
                        && copyMetadata(metadataSource, stagedPath, true)
                        && (!publishGuard || publishGuard()))
                    {
                        std::error_code sizeError;
                        const auto stagedSize = std::filesystem::file_size(stagedPath, sizeError);
                        constexpr std::uintmax_t metadataGrowthLimit = 80ULL * 1024ULL * 1024ULL;
                        const auto maximum = std::numeric_limits<std::uintmax_t>::max();
                        const auto encodedSize = static_cast<std::uintmax_t>(buffer.size());
                        const auto stagedLimit = encodedSize > maximum - metadataGrowthLimit
                                                     ? maximum
                                                     : encodedSize + metadataGrowthLimit;
                        std::ifstream input(stagedPath, std::ios::binary);
                        if (!sizeError && stagedSize > 0 && stagedSize <= stagedLimit && input)
                        {
                            std::vector<uchar>().swap(buffer);
                            const bool saved = writeTemporaryAndPublishAtRoot(
                                outputRoot,
                                relativeDestination,
                                [&input, &publishGuard](const int descriptor)
                                {
                                    return writeStream(descriptor, input, publishGuard);
                                },
                                publishGuard);
                            return saved ? ImageWriteResult::Saved : ImageWriteResult::Failed;
                        }
                    }
                }
            }
        }

        const bool saved = writeTemporaryAndPublishAtRoot(
            outputRoot,
            relativeDestination,
            [&buffer](const int descriptor)
            {
                return writeAll(descriptor, buffer.data(), buffer.size());
            },
            publishGuard);
        if (!saved)
        {
            return ImageWriteResult::Failed;
        }
        return metadataSource.empty() ? ImageWriteResult::Saved
                                      : ImageWriteResult::SavedWithoutMetadata;
    }

    bool copyFileNoReplace(
        const std::filesystem::path &source, const std::filesystem::path &destination)
    {
        std::error_code ec;
        const auto absoluteDestination = std::filesystem::absolute(destination, ec);
        if (ec)
        {
            return false;
        }
        const auto outputRoot = std::filesystem::canonical(absoluteDestination.parent_path(), ec);
        if (ec)
        {
            return false;
        }
        return copyFileNoReplaceAtRoot(source, outputRoot, absoluteDestination.filename());
    }

    bool copyFileNoReplaceAtRoot(const std::filesystem::path &source,
        const std::filesystem::path &outputRoot,
        const std::filesystem::path &relativeDestination,
        const std::function<bool()> &publishGuard)
    {
        std::error_code canonicalError;
        const auto canonicalSource = std::filesystem::canonical(source, canonicalError);
        if (canonicalError)
        {
            return false;
        }
        const auto initialIdentity = captureFileIdentity(canonicalSource);
        if (!initialIdentity)
        {
            return false;
        }
        FileDescriptor input = openReadOnlyDescriptor(canonicalSource);
        const auto openedIdentity =
            input.valid() ? descriptorIdentity(input.get()) : std::optional<FileIdentity>{};
        if (!openedIdentity || *openedIdentity != *initialIdentity)
        {
            return false;
        }

        const auto sourceIsStable = [&]
        {
            const auto pathIdentity = captureFileIdentity(canonicalSource);
            const auto currentOpenedIdentity = descriptorIdentity(input.get());
            return pathIdentity && *pathIdentity == *initialIdentity && currentOpenedIdentity
                   && *currentOpenedIdentity == *initialIdentity
                   && (!publishGuard || publishGuard());
        };

        return writeTemporaryAndPublishAtRoot(
            outputRoot,
            relativeDestination,
            [&input, &sourceIsStable](const int descriptor)
            {
                return copyDescriptor(input.get(), descriptor, sourceIsStable);
            },
            sourceIsStable);
    }

    FileMoveResult moveFileNoReplaceAtRoot(const std::filesystem::path &source,
        const std::filesystem::path &outputRoot,
        const std::filesystem::path &relativeDestination,
        const std::function<bool()> &publishGuard)
    {
        std::error_code canonicalError;
        const auto canonicalSource = std::filesystem::canonical(source, canonicalError);
        if (canonicalError)
        {
            return FileMoveResult::Failed;
        }
        const auto initialIdentity = captureFileIdentity(canonicalSource);
        if (!initialIdentity)
        {
            return FileMoveResult::Failed;
        }

#ifdef _WIN32
        auto parent = openWindowsParent(outputRoot, relativeDestination);
        if (!parent)
        {
            return FileMoveResult::Failed;
        }

        const HANDLE sourceHandle = ::CreateFileW(canonicalSource.c_str(),
            GENERIC_READ | GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES | WRITE_DAC,
            0,
            nullptr,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH,
            nullptr);
        if (sourceHandle == INVALID_HANDLE_VALUE)
        {
            return FileMoveResult::Failed;
        }
        const int sourceDescriptor =
            ::_open_osfhandle(reinterpret_cast<intptr_t>(sourceHandle), _O_BINARY | _O_RDWR);
        if (sourceDescriptor < 0)
        {
            ::CloseHandle(sourceHandle);
            return FileMoveResult::Failed;
        }
        FileDescriptor input(sourceDescriptor);
        const auto openedIdentity = descriptorIdentity(input.get());
        BY_HANDLE_FILE_INFORMATION openedInfo{};
        const HANDLE openedHandle = reinterpret_cast<HANDLE>(::_get_osfhandle(input.get()));
        if (!openedIdentity || *openedIdentity != *initialIdentity
            || openedHandle == INVALID_HANDLE_VALUE
            || !::GetFileInformationByHandle(openedHandle, &openedInfo)
            || (openedInfo.dwFileAttributes
                   & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT))
                   != 0
            || openedInfo.nNumberOfLinks != 1 || !syncDescriptor(input.get())
            || (publishGuard && !publishGuard()))
        {
            return FileMoveResult::Failed;
        }

        PSECURITY_DESCRIPTOR descriptor = privateSecurityDescriptor();
        PACL privateDacl = nullptr;
        BOOL daclPresent = FALSE;
        BOOL daclDefaulted = FALSE;
        const bool privateAclApplied =
            descriptor != nullptr
            && ::GetSecurityDescriptorDacl(descriptor, &daclPresent, &privateDacl, &daclDefaulted)
                   != 0
            && daclPresent && privateDacl != nullptr
            && ::SetSecurityInfo(openedHandle,
                   SE_FILE_OBJECT,
                   DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                   nullptr,
                   nullptr,
                   privateDacl,
                   nullptr)
                   == ERROR_SUCCESS;
        if (descriptor != nullptr)
        {
            ::LocalFree(descriptor);
        }
        if (!privateAclApplied)
        {
            return FileMoveResult::Failed;
        }

        const std::wstring destinationName = (parent->path / parent->filename).native();
        const std::size_t nameBytes = destinationName.size() * sizeof(wchar_t);
        if (nameBytes > std::numeric_limits<DWORD>::max())
        {
            return FileMoveResult::Failed;
        }
        const std::size_t infoSize = sizeof(FILE_RENAME_INFO) + nameBytes;
        if (infoSize > std::numeric_limits<DWORD>::max())
        {
            return FileMoveResult::Failed;
        }
        const std::size_t storageCount =
            (infoSize + sizeof(std::max_align_t) - 1) / sizeof(std::max_align_t);
        std::vector<std::max_align_t> storage(storageCount);
        std::memset(storage.data(), 0, storage.size() * sizeof(std::max_align_t));
        auto *renameInfo = reinterpret_cast<FILE_RENAME_INFO *>(storage.data());
        renameInfo->RootDirectory = nullptr;
        renameInfo->FileNameLength = static_cast<DWORD>(nameBytes);
        std::memcpy(renameInfo->FileName, destinationName.data(), nameBytes);
        if (publishGuard && !publishGuard())
        {
            return FileMoveResult::Failed;
        }
        if (::SetFileInformationByHandle(
                openedHandle, FileRenameInfo, renameInfo, static_cast<DWORD>(infoSize))
            != 0)
        {
            return FileMoveResult::Moved;
        }
        return ::GetLastError() == ERROR_NOT_SAME_DEVICE ? FileMoveResult::CrossDevice
                                                         : FileMoveResult::Failed;
#else
        FileDescriptor input = openReadOnlyDescriptor(canonicalSource);
        const auto openedIdentity =
            input.valid() ? descriptorIdentity(input.get()) : std::optional<FileIdentity>{};
        if (!openedIdentity || *openedIdentity != *initialIdentity)
        {
            return FileMoveResult::Failed;
        }

        PosixDescriptor sourceParent(
            ::open(canonicalSource.parent_path().c_str(), directoryOpenFlags()));
        auto destinationParent = openPosixParent(outputRoot, relativeDestination);
        if (!sourceParent.valid() || !destinationParent)
        {
            return FileMoveResult::Failed;
        }
        const auto sourceName = canonicalSource.filename().native();
        struct stat openedInfo{};
        struct stat namedInfo{};
        struct stat destinationInfo{};
        const auto pathIdentity = captureFileIdentity(canonicalSource);
        const auto currentIdentity = descriptorIdentity(input.get());
        if (::fstat(input.get(), &openedInfo) != 0
            || ::fstatat(sourceParent.get(), sourceName.c_str(), &namedInfo, AT_SYMLINK_NOFOLLOW)
                   != 0
            || !pathIdentity || !currentIdentity || *pathIdentity != *initialIdentity
            || *currentIdentity != *initialIdentity || !S_ISREG(namedInfo.st_mode)
            || namedInfo.st_nlink != 1 || openedInfo.st_dev != namedInfo.st_dev
            || openedInfo.st_ino != namedInfo.st_ino)
        {
            return FileMoveResult::Failed;
        }
        if (::fstatat(destinationParent->descriptor.get(),
                destinationParent->filename.c_str(),
                &destinationInfo,
                AT_SYMLINK_NOFOLLOW)
                == 0
            || errno != ENOENT || (publishGuard && !publishGuard()))
        {
            return FileMoveResult::Failed;
        }

        const mode_t originalMode = openedInfo.st_mode & 07777;
        if (::fchmod(input.get(), 0600) != 0)
        {
            return FileMoveResult::Failed;
        }
        if (!syncDescriptor(input.get()))
        {
            ::fchmod(input.get(), originalMode);
            return FileMoveResult::Failed;
        }
        if (publishGuard && !publishGuard())
        {
            ::fchmod(input.get(), originalMode);
            return FileMoveResult::Failed;
        }

        if (publishPosixNoReplace(sourceParent.get(),
                sourceName.c_str(),
                destinationParent->descriptor.get(),
                destinationParent->filename.c_str(),
                publishGuard))
        {
            ::fsync(destinationParent->descriptor.get());
            ::fsync(sourceParent.get());
            return FileMoveResult::Moved;
        }
        const int publishError = errno;
        ::fchmod(input.get(), originalMode);
        return publishError == EXDEV ? FileMoveResult::CrossDevice : FileMoveResult::Failed;
#endif
    }

    std::vector<int> encodeParamsForExtension(const std::string &extLower)
    {
        std::string ext = extLower;
        std::transform(ext.begin(),
            ext.end(),
            ext.begin(),
            [](unsigned char c)
            {
                return static_cast<char>(std::tolower(c));
            });
        if (!ext.empty() && ext.front() == '.')
        {
            ext.erase(ext.begin());
        }

        if (ext == "jpg" || ext == "jpeg")
        {
            std::vector<int> params = {cv::IMWRITE_JPEG_QUALITY, 100};
#if CV_VERSION_MAJOR > 4 || (CV_VERSION_MAJOR == 4 && CV_VERSION_MINOR >= 7)
            params.push_back(cv::IMWRITE_JPEG_SAMPLING_FACTOR);
            params.push_back(cv::IMWRITE_JPEG_SAMPLING_FACTOR_444);
#endif
            params.push_back(cv::IMWRITE_JPEG_OPTIMIZE);
            params.push_back(1);
            return params;
        }
        if (ext == "webp")
        {
            return {cv::IMWRITE_WEBP_QUALITY, 101};
        }
        if (ext == "png")
        {
            return {cv::IMWRITE_PNG_COMPRESSION, 6};
        }
        if (ext == "tif" || ext == "tiff")
        {
            return {cv::IMWRITE_TIFF_COMPRESSION, 5};
        }
        return {};
    }

    bool copyMetadata(const std::filesystem::path &source,
        const std::filesystem::path &destination,
        bool normalizeOrientation)
    {
#ifdef CLOAKFRAME_HAVE_EXIV2
        if (!initializeExiv2())
        {
            return false;
        }
        const std::lock_guard<std::mutex> lock(g_exiv2Mutex);
        try
        {
            auto src = Exiv2::ImageFactory::open(toUtf8(source));
            src->readMetadata();

            auto dst = Exiv2::ImageFactory::open(toUtf8(destination));
            dst->readMetadata();

            Exiv2::ExifData exif = src->exifData();
            Exiv2::ExifThumb thumb(exif);
            thumb.erase();
            constexpr std::size_t metadataEntryLimit = 128U;
            constexpr std::size_t metadataTotalLimit = std::size_t{64} * 1024;
            std::size_t exifBytes = 0;
            for (auto it = exif.begin(); it != exif.end();)
            {
                std::string key = it->key();
                std::ranges::transform(key,
                    key.begin(),
                    [](const unsigned char value)
                    {
                        return static_cast<char>(std::tolower(value));
                    });
                const bool standardGroup =
                    key.starts_with("exif.image.") || key.starts_with("exif.photo.")
                    || key.starts_with("exif.gpsinfo.") || key.starts_with("exif.iop.");
                constexpr std::array<std::string_view, 25> blocked = {
                    "thumbnail",
                    "preview",
                    "makernote",
                    "stripoffset",
                    "stripbyte",
                    "tileoffset",
                    "tilebyte",
                    "jpeginterchange",
                    "jpgfromraw",
                    "otherimage",
                    "originalraw",
                    "dngprivate",
                    "opcode",
                    "subifd",
                    "subimage",
                    "imageoffset",
                    "imagebytecount",
                    "imagewidth",
                    "imagelength",
                    "pixeldimension",
                    "xmlpacket",
                    "applicationnotes",
                    "imageresource",
                    "photoshop",
                    "intercolorprofile",
                };
                const bool unsafe = std::ranges::any_of(blocked,
                    [&](const auto value)
                    {
                        return key.find(value) != std::string::npos;
                    });
                const auto bytes = it->size();
                const auto type = it->typeId();
                const bool safeType = type == Exiv2::unsignedByte || type == Exiv2::asciiString
                                      || type == Exiv2::unsignedShort || type == Exiv2::unsignedLong
                                      || type == Exiv2::unsignedRational
                                      || type == Exiv2::signedByte || type == Exiv2::signedShort
                                      || type == Exiv2::signedLong || type == Exiv2::signedRational
                                      || type == Exiv2::tiffFloat || type == Exiv2::tiffDouble;
                if (!standardGroup || unsafe || !safeType || bytes > metadataEntryLimit
                    || exifBytes > metadataTotalLimit - bytes)
                {
                    it = exif.erase(it);
                }
                else
                {
                    exifBytes += bytes;
                    ++it;
                }
            }
            if (normalizeOrientation && !exif.empty())
            {
                exif["Exif.Image.Orientation"] = static_cast<uint16_t>(1);
            }
            dst->setExifData(exif);
            dst->setIptcData(Exiv2::IptcData{});
            dst->setXmpData(Exiv2::XmpData{});
            dst->clearIccProfile();
            dst->clearComment();

            dst->writeMetadata();
            return true;
        }
        catch (const std::exception &)
        {
            return false;
        }
#else
        (void)source;
        (void)destination;
        (void)normalizeOrientation;
        return false;
#endif
    }
}
