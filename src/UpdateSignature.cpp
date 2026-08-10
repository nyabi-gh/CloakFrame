#include "cloakframe/UpdateSignature.hpp"

#include <QCryptographicHash>
#include <QFile>

#include <mutex>
#include <sodium.h>
#include <utility>

namespace cloakframe
{
    namespace
    {
        void setError(QString *error, const QString &message)
        {
            if (error != nullptr)
            {
                *error = message;
            }
        }

        // sodium_init() is documented as unsafe to call from several threads at once, and the
        // updater runs on its own thread.
        bool sodiumReady()
        {
            static std::once_flag once;
            static bool ready = false;
            std::call_once(once,
                []
                {
                    ready = sodium_init() >= 0;
                });
            return ready;
        }

        std::optional<QByteArray> decodeExactly(
            const QString &base64, const int expectedSize, const char *what, QString *error)
        {
            auto result = QByteArray::fromBase64Encoding(base64.toUtf8(),
                QByteArray::Base64Encoding | QByteArray::AbortOnBase64DecodingErrors);
            if (!result)
            {
                setError(error, QStringLiteral("update %1 is not valid base64").arg(what));
                return std::nullopt;
            }
            QByteArray bytes = std::move(result.decoded);
            if (bytes.size() != expectedSize)
            {
                setError(error,
                    QStringLiteral("update %1 is %2 bytes, expected %3")
                        .arg(what)
                        .arg(bytes.size())
                        .arg(expectedSize));
                return std::nullopt;
            }
            return bytes;
        }
    }

    bool verifyUpdateSignature(const QByteArray &payload,
        const QString &signatureBase64,
        const QString &publicKeyBase64,
        QString *error)
    {
        if (!sodiumReady())
        {
            setError(error, QStringLiteral("could not initialise the signature library"));
            return false;
        }
        if (payload.isEmpty())
        {
            setError(error, QStringLiteral("nothing to verify"));
            return false;
        }

        const auto key =
            decodeExactly(publicKeyBase64, crypto_sign_PUBLICKEYBYTES, "public key", error);
        if (!key)
        {
            return false;
        }
        const auto signature =
            decodeExactly(signatureBase64, crypto_sign_BYTES, "signature", error);
        if (!signature)
        {
            return false;
        }

        if (crypto_sign_verify_detached(
                reinterpret_cast<const unsigned char *>(signature->constData()),
                reinterpret_cast<const unsigned char *>(payload.constData()),
                static_cast<unsigned long long>(payload.size()),
                reinterpret_cast<const unsigned char *>(key->constData()))
            != 0)
        {
            setError(error, QStringLiteral("the update is not signed by the key this build pins"));
            return false;
        }
        return true;
    }

    std::optional<QByteArray> sha256HexOfFile(const QString &path, QString *error)
    {
        QFile file(path);
        if (!file.open(QIODevice::ReadOnly))
        {
            setError(error, QStringLiteral("cannot read %1: %2").arg(path, file.errorString()));
            return std::nullopt;
        }

        QCryptographicHash hash(QCryptographicHash::Sha256);
        if (!hash.addData(&file))
        {
            setError(error, QStringLiteral("cannot read %1: %2").arg(path, file.errorString()));
            return std::nullopt;
        }
        return hash.result().toHex();
    }
}
