#include "cloakframe/ModelDownload.hpp"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QTcpServer>
#include <QTcpSocket>
#include <QTimer>

#include <cassert>

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    QTcpServer server;
    assert(server.listen(QHostAddress::LocalHost));
    const QUrl url(QStringLiteral("http://127.0.0.1:%1/model").arg(server.serverPort()));
    const QByteArray bytes("model-data");
    const QString hash =
        QString::fromLatin1(QCryptographicHash::hash(bytes, QCryptographicHash::Sha256).toHex());
    int requests = 0;
    enum class Mode
    {
        Success,
        Retry,
        Stall,
        Trickle,
        Huge,
        BadHash,
        NotFound
    };
    Mode mode = Mode::Success;
    QObject::connect(&server,
        &QTcpServer::newConnection,
        &server,
        [&]
        {
            auto *socket = server.nextPendingConnection();
            QObject::connect(socket, &QTcpSocket::disconnected, socket, &QObject::deleteLater);
            QObject::connect(socket,
                &QTcpSocket::readyRead,
                socket,
                [&, socket]
                {
                    socket->readAll();
                    if (socket->property("responded").toBool())
                        return;
                    socket->setProperty("responded", true);
                    ++requests;
                    if (mode == Mode::Stall)
                        return;
                    if (mode == Mode::Retry && requests == 1)
                        socket->write("HTTP/1.1 503 Unavailable\r\nContent-Length: "
                                      "0\r\nConnection: close\r\n\r\n");
                    else if (mode == Mode::NotFound)
                        socket->write("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: "
                                      "close\r\n\r\n");
                    else if (mode == Mode::Huge)
                        socket->write("HTTP/1.1 200 OK\r\nContent-Length: 9999999\r\n\r\n");
                    else if (mode == Mode::Trickle)
                    {
                        socket->write("HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n");
                        auto *timer = new QTimer(socket);
                        QObject::connect(timer,
                            &QTimer::timeout,
                            socket,
                            [socket]
                            {
                                socket->write("x");
                            });
                        timer->start(10);
                        return;
                    }
                    else
                        socket->write(
                            "HTTP/1.1 200 OK\r\nContent-Length: " + QByteArray::number(bytes.size())
                            + "\r\nConnection: close\r\n\r\n" + bytes);
                    socket->disconnectFromHost();
                });
        });
    using cloakframe::ModelDownloadStatus;
    const auto run = [&](Mode selected, bool cancel = false)
    {
        mode = selected;
        requests = 0;
        return cloakframe::downloadModelData(url,
            2000,
            selected == Mode::BadHash ? QStringLiteral("bad") : hash,
            [cancel]
            {
                return cancel;
            },
            {},
            {60, 200, 3, 5});
    };
    auto result = run(Mode::Success);
    assert(result.status == ModelDownloadStatus::Success && result.data == bytes
           && result.attempts == 1);
    result = run(Mode::Retry);
    assert(result.status == ModelDownloadStatus::Success && result.attempts == 2);
    result = run(Mode::Stall);
    assert(result.status == ModelDownloadStatus::TimedOut && result.data.isEmpty());
    result = run(Mode::Trickle);
    assert(result.status == ModelDownloadStatus::TimedOut);
    result = run(Mode::Huge);
    assert(result.status == ModelDownloadStatus::TooLarge && result.attempts == 1);
    result = run(Mode::BadHash);
    assert(result.status == ModelDownloadStatus::IntegrityError && result.data.isEmpty()
           && result.attempts == 1);
    result = run(Mode::NotFound);
    assert(result.status == ModelDownloadStatus::NetworkError && result.attempts == 1);
    result = run(Mode::Success, true);
    assert(result.status == ModelDownloadStatus::Cancelled && result.attempts == 0);
    mode = Mode::Stall;
    bool cancelled = false;
    QTimer::singleShot(30,
        &app,
        [&]
        {
            cancelled = true;
        });
    result = cloakframe::downloadModelData(url,
        2000,
        hash,
        [&]
        {
            return cancelled;
        },
        {},
        {500, 1000, 3, 5});
    assert(result.status == ModelDownloadStatus::Cancelled && result.attempts == 1);
    mode = Mode::Retry;
    requests = 0;
    cancelled = false;
    QTimer::singleShot(30,
        &app,
        [&]
        {
            cancelled = true;
        });
    result = cloakframe::downloadModelData(url,
        2000,
        hash,
        [&]
        {
            return cancelled;
        },
        {},
        {500, 1000, 3, 200});
    assert(result.status == ModelDownloadStatus::Cancelled && result.attempts == 1);
    return 0;
}
