#include "cloakframe/ModelStore.hpp"

#include "cloakframe/UpdateCache.hpp"

#include <QByteArrayView>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QRandomGenerator>

#include <algorithm>
#include <cerrno>
#include <climits>
#include <cstddef>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#include <windows.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif

namespace cloakframe
{
    namespace
    {
        constexpr int kOpenAttempts = 16;
        constexpr qint64 kReadChunk = 1 << 20;

        class Descriptor
        {
        public:
            explicit Descriptor(int descriptor)
                : descriptor_(descriptor)
            {
            }

            ~Descriptor()
            {
                close();
            }

            Descriptor(const Descriptor &) = delete;
            Descriptor &operator=(const Descriptor &) = delete;

            [[nodiscard]] bool valid() const
            {
                return descriptor_ >= 0;
            }

            [[nodiscard]] int get() const
            {
                return descriptor_;
            }

            void close()
            {
                if (descriptor_ < 0)
                {
                    return;
                }
#ifdef _WIN32
                ::_close(descriptor_);
#else
                ::close(descriptor_);
#endif
                descriptor_ = -1;
            }

        private:
            int descriptor_ = -1;
        };

        QString temporaryName(const QString &fileName)
        {
            const auto suffix = QString::number(QRandomGenerator::system()->generate64(), 16);
            return fileName + "." + suffix + ".part";
        }

        // Exclusive creation is what makes the name safe to use: it fails outright if anything
        // already answers to it, a planted link included.
        int createExclusive(const QString &path)
        {
#ifdef _WIN32
            const auto native = QDir::toNativeSeparators(path).toStdWString();
            const HANDLE handle = ::CreateFileW(native.c_str(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                nullptr,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                nullptr);
            if (handle == INVALID_HANDLE_VALUE)
            {
                return -1;
            }
            const auto raw = reinterpret_cast<intptr_t>(handle);
            const int descriptor = ::_open_osfhandle(raw, _O_BINARY | _O_RDWR);
            if (descriptor < 0)
            {
                ::CloseHandle(handle);
            }
            return descriptor;
#else
            int flags = O_CREAT | O_EXCL | O_RDWR;
#ifdef O_CLOEXEC
            flags |= O_CLOEXEC;
#endif
#ifdef O_NOFOLLOW
            flags |= O_NOFOLLOW;
#endif
            return ::open(QFile::encodeName(path).constData(), flags, 0600);
#endif
        }

        bool writeAll(const int descriptor, const QByteArray &bytes)
        {
            const char *cursor = bytes.constData();
            auto remaining = static_cast<std::size_t>(bytes.size());
            while (remaining > 0)
            {
#ifdef _WIN32
                const auto chunk = static_cast<unsigned int>(
                    std::min<std::size_t>(remaining, static_cast<std::size_t>(INT_MAX)));
                const int written = ::_write(descriptor, cursor, chunk);
#else
                const ssize_t written = ::write(descriptor, cursor, remaining);
#endif
                if (written < 0)
                {
                    if (errno == EINTR)
                    {
                        continue;
                    }
                    return false;
                }
                cursor += written;
                remaining -= static_cast<std::size_t>(written);
            }
            return true;
        }

        bool syncDescriptor(const int descriptor)
        {
#ifdef _WIN32
            return ::_commit(descriptor) == 0;
#else
            return ::fsync(descriptor) == 0;
#endif
        }

        // Hashing through the descriptor the bytes were written to, rather than reopening the
        // path, is what makes this a check on the file that is about to be published instead of
        // on whatever the name happens to point at by then.
        QByteArray digestOfDescriptor(const int descriptor)
        {
#ifdef _WIN32
            if (::_lseeki64(descriptor, 0, SEEK_SET) != 0)
#else
            if (::lseek(descriptor, 0, SEEK_SET) != 0)
#endif
            {
                return {};
            }

            QCryptographicHash hash(QCryptographicHash::Sha256);
            QByteArray buffer(kReadChunk, Qt::Uninitialized);
            while (true)
            {
#ifdef _WIN32
                const int got =
                    ::_read(descriptor, buffer.data(), static_cast<unsigned int>(kReadChunk));
#else
                const ssize_t got =
                    ::read(descriptor, buffer.data(), static_cast<std::size_t>(kReadChunk));
#endif
                if (got < 0)
                {
                    if (errno == EINTR)
                    {
                        continue;
                    }
                    return {};
                }
                if (got == 0)
                {
                    break;
                }
                hash.addData(QByteArrayView(buffer.constData(), got));
            }
            return hash.result().toHex();
        }

        bool replaceWith(const QString &temporaryPath, const QString &destPath)
        {
#ifdef _WIN32
            return ::MoveFileExW(QDir::toNativeSeparators(temporaryPath).toStdWString().c_str(),
                       QDir::toNativeSeparators(destPath).toStdWString().c_str(),
                       MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)
                   != 0;
#else
            // rename() replaces in one step, so the model already in place stays readable until
            // the moment the new one takes over and survives untouched if this fails.
            return ::rename(QFile::encodeName(temporaryPath).constData(),
                       QFile::encodeName(destPath).constData())
                   == 0;
#endif
        }

        void syncDirectory(const QString &path)
        {
#ifndef _WIN32
            Descriptor directory(::open(QFile::encodeName(path).constData(), O_RDONLY));
            if (directory.valid())
            {
                ::fsync(directory.get());
            }
#else
            Q_UNUSED(path);
#endif
        }
    }

    ModelSaveResult saveModelFile(
        const QString &destPath, const QByteArray &bytes, const QString &expectedSha256Hex)
    {
        const QFileInfo destination(destPath);
        const QString folder = destination.absolutePath();
        if (!QDir().mkpath(folder))
        {
            return ModelSaveResult::WriteFailed;
        }
        if (inspectCacheDirectory(folder) == CacheDirectoryTrust::Shared)
        {
            return ModelSaveResult::FolderNotPrivate;
        }

        QString temporaryPath;
        int opened = -1;
        for (int attempt = 0; attempt < kOpenAttempts && opened < 0; ++attempt)
        {
            temporaryPath = folder + "/" + temporaryName(destination.fileName());
            opened = createExclusive(temporaryPath);
        }
        if (opened < 0)
        {
            return ModelSaveResult::WriteFailed;
        }
        Descriptor file(opened);

        const auto discard = [&file, &temporaryPath](ModelSaveResult result)
        {
            file.close();
            QFile::remove(temporaryPath);
            return result;
        };

        if (!writeAll(file.get(), bytes) || !syncDescriptor(file.get()))
        {
            return discard(ModelSaveResult::WriteFailed);
        }

        const auto actual = digestOfDescriptor(file.get());
        if (actual.isEmpty()
            || QString::fromLatin1(actual).compare(expectedSha256Hex, Qt::CaseInsensitive) != 0)
        {
            return discard(ModelSaveResult::ContentMismatch);
        }

        file.close();
        if (!replaceWith(temporaryPath, destPath))
        {
            QFile::remove(temporaryPath);
            return ModelSaveResult::PublishFailed;
        }
        syncDirectory(folder);
        return ModelSaveResult::Saved;
    }
}
