#include "cloakframe/Logging.hpp"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>

#include <spdlog/sinks/base_sink.h>
#include <spdlog/sinks/ostream_sink.h>
#include <spdlog/sinks/rotating_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>
#include <spdlog/spdlog.h>

#include <mutex>
#include <ostream>

namespace cloakframe
{
    namespace
    {
        // The one place that decides what leaves the process. Standard output is a record too:
        // desktop sessions route it into the system journal. So the console gets exactly what
        // the file gets.
        class PrivateLogSink final : public spdlog::sinks::base_sink<std::mutex>
        {
        public:
            PrivateLogSink(QString directory, bool detailed, spdlog::sink_ptr console)
                : directory_(std::move(directory))
                , detailed_(detailed)
                , console_(std::move(console))
            {
                open();
            }
            void setDetailed(bool enabled)
            {
                const std::lock_guard lock(mutex_);
                detailed_ = enabled;
            }
            void diagnostic(const QString &message)
            {
                const auto bytes = message.toUtf8();
                const spdlog::details::log_msg entry("diagnostic",
                    spdlog::level::info,
                    spdlog::string_view_t(bytes.constData(), static_cast<size_t>(bytes.size())));
                const std::lock_guard lock(mutex_);
                if (file_)
                {
                    file_->log(entry);
                    file_->flush();
                }
                if (console_)
                {
                    console_->log(entry);
                    console_->flush();
                }
            }
            bool clear()
            {
                const std::lock_guard lock(mutex_);
                file_.reset();
                bool success = true;
                for (const auto &name :
                    {"cloakframe.log", "cloakframe.1.log", "cloakframe.2.log", "cloakframe.3.log"})
                {
                    const QString path = QDir(directory_).filePath(name);
                    if (QFileInfo::exists(path) || QFileInfo(path).isSymLink())
                        success = QFile::remove(path) && success;
                }
                open();
                return success && file_ != nullptr;
            }

        protected:
            void sink_it_(const spdlog::details::log_msg &message) override
            {
                if (!detailed_)
                    return;
                if (file_)
                {
                    file_->log(message);
                    file_->flush();
                }
                if (console_)
                    console_->log(message);
            }
            void flush_() override
            {
                if (file_)
                    file_->flush();
                if (console_)
                    console_->flush();
            }

        private:
            void open()
            {
                try
                {
                    if (directory_.isEmpty() || !QDir().mkpath(directory_))
                        return;
                    for (const auto &name : {"cloakframe.log",
                             "cloakframe.1.log",
                             "cloakframe.2.log",
                             "cloakframe.3.log"})
                        if (QFileInfo(QDir(directory_).filePath(name)).isSymLink())
                            return;
                    file_ = std::make_shared<spdlog::sinks::rotating_file_sink_mt>(
                        QDir(directory_).filePath("cloakframe.log").toStdString(), 1024 * 1024, 3);
                    file_->set_pattern("[%Y-%m-%d %H:%M:%S] [%l] %v");
                }
                catch (const std::exception &)
                {
                    file_.reset();
                }
            }
            QString directory_;
            bool detailed_;
            spdlog::sink_ptr console_;
            std::shared_ptr<spdlog::sinks::rotating_file_sink_mt> file_;
        };
        std::shared_ptr<PrivateLogSink> localSink;
        std::ostream *consoleOverride = nullptr;
    }
    QString localLogDirectory()
    {
        const QString base = QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation);
        return base.isEmpty() ? QString() : base + "/CloakFrame/logs";
    }
    void configureLogging(const QString &directory, bool detailed)
    {
        spdlog::sink_ptr console;
        if (consoleOverride != nullptr)
            console = std::make_shared<spdlog::sinks::ostream_sink_mt>(*consoleOverride);
        else
            console = std::make_shared<spdlog::sinks::stdout_color_sink_mt>();
        localSink = std::make_shared<PrivateLogSink>(directory, detailed, std::move(console));
        auto logger = std::make_shared<spdlog::logger>("cloakframe", localSink);
        logger->flush_on(spdlog::level::info);
        spdlog::set_default_logger(logger);
        spdlog::set_level(spdlog::level::info);
        logDiagnostic(QStringLiteral("Application started"));
    }
    void setConsoleLogStreamForTesting(std::ostream *stream)
    {
        consoleOverride = stream;
    }
    void setDetailedLogging(bool enabled)
    {
        if (localSink)
            localSink->setDetailed(enabled);
    }
    bool clearLocalLogs()
    {
        return localSink && localSink->clear();
    }
    void logDiagnostic(const QString &message)
    {
        if (localSink)
            localSink->diagnostic(message);
    }
}
