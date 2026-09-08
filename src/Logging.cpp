#include "cloakframe/Logging.hpp"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>

#include <spdlog/sinks/base_sink.h>
#include <spdlog/sinks/rotating_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>
#include <spdlog/spdlog.h>

#include <mutex>

namespace cloakframe
{
    namespace
    {
        class PrivateLogSink final : public spdlog::sinks::base_sink<std::mutex>
        {
        public:
            PrivateLogSink(QString directory, bool detailed)
                : directory_(std::move(directory))
                , detailed_(detailed)
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
                if (detailed_ && file_)
                {
                    file_->log(message);
                    file_->flush();
                }
            }
            void flush_() override
            {
                if (file_)
                    file_->flush();
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
            std::shared_ptr<spdlog::sinks::rotating_file_sink_mt> file_;
        };
        std::shared_ptr<PrivateLogSink> localSink;
    }
    QString localLogDirectory()
    {
        const QString base = QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation);
        return base.isEmpty() ? QString() : base + "/CloakFrame/logs";
    }
    void configureLogging(const QString &directory, bool detailed)
    {
        localSink = std::make_shared<PrivateLogSink>(directory, detailed);
        auto console = std::make_shared<spdlog::sinks::stdout_color_sink_mt>();
        auto logger = std::make_shared<spdlog::logger>(
            "cloakframe", spdlog::sinks_init_list{console, localSink});
        logger->flush_on(spdlog::level::info);
        spdlog::set_default_logger(logger);
        spdlog::set_level(spdlog::level::info);
        logDiagnostic(QStringLiteral("Application started"));
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
