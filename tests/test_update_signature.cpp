#include "cloakframe/UpdateSignature.hpp"

#include <QByteArray>
#include <QString>
#include <QTemporaryDir>

#include <cassert>
#include <cstdio>
#include <fstream>

namespace
{
    // RFC 8032 section 7.1, test vector 2: a one-byte message, so the vector proves the
    // verification itself rather than anything about how CloakFrame frames a payload.
    const QString kRfcPublicKey = QStringLiteral("PUAXw+hDiVqStwqnTRt+vJyYLM8uxJaMwM1V8Sr0Zgw=");
    const QString kRfcSignature =
        QStringLiteral("kqAJqfDUyrhyDoILX2QlQKKye1QWUD+Ps3YiI+vbadoIWsHkPhWZbkWPNhPQ8R2MOHsurrQwK"
                       "u6wDSkWErsMAA==");
    const QByteArray kRfcMessage = QByteArray::fromHex("72");

    // The shape the release workflow actually produces: an Ed25519 signature over the lowercase
    // hex SHA-256 of a file, made with `openssl pkeyutl -sign -rawin`. The digest is of "abc".
    const QString kRealPublicKey = QStringLiteral("b8LBqH0YWEKwE6iZJ+gqey5I13A/rT8srBWBUSrICPQ=");
    const QString kRealSignature =
        QStringLiteral("z1TZzXjZnAH2z9T5z7Ul1wlN5SrLos/FjdN87mVX6Vc0hCI1V7q82NIdhNEBjaupJAsDVVN0b"
                       "zgULbVZR5hNAQ==");
    const QByteArray kAbcDigest =
        QByteArrayLiteral("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");

    void testRfcVectorVerifies()
    {
        QString error = QStringLiteral("untouched");
        assert(
            cloakframe::verifyUpdateSignature(kRfcMessage, kRfcSignature, kRfcPublicKey, &error));
        assert(error == QStringLiteral("untouched"));
    }

    void testWorkflowShapeVerifies()
    {
        assert(cloakframe::verifyUpdateSignature(kAbcDigest, kRealSignature, kRealPublicKey));
    }

    void testTamperedPayloadFails()
    {
        QByteArray tampered = kAbcDigest;
        tampered[0] = 'c';
        QString error;
        assert(
            !cloakframe::verifyUpdateSignature(tampered, kRealSignature, kRealPublicKey, &error));
        assert(!error.isEmpty());
    }

    void testOtherKeyFails()
    {
        // A signature that verifies under its own key must not verify under another real key.
        assert(!cloakframe::verifyUpdateSignature(kAbcDigest, kRealSignature, kRfcPublicKey));
        assert(!cloakframe::verifyUpdateSignature(kRfcMessage, kRfcSignature, kRealPublicKey));
    }

    void testTamperedSignatureFails()
    {
        QByteArray raw = QByteArray::fromBase64(kRealSignature.toUtf8());
        raw[0] = static_cast<char>(raw[0] ^ 0x01);
        assert(!cloakframe::verifyUpdateSignature(
            kAbcDigest, QString::fromUtf8(raw.toBase64()), kRealPublicKey));
    }

    void testMalformedInputIsRejected()
    {
        // Every one of these has to fail closed: a build that cannot parse what it was given
        // knows nothing about the update, which is not the same as the update being genuine.
        QString error;
        assert(!cloakframe::verifyUpdateSignature(
            kAbcDigest, QStringLiteral("not base64!!"), kRealPublicKey, &error));
        assert(!error.isEmpty());

        assert(!cloakframe::verifyUpdateSignature(
            kAbcDigest, kRealSignature, QStringLiteral("not base64!!")));
        // Right encoding, wrong length.
        assert(
            !cloakframe::verifyUpdateSignature(kAbcDigest, kRealSignature, QStringLiteral("YWJj")));
        assert(
            !cloakframe::verifyUpdateSignature(kAbcDigest, QStringLiteral("YWJj"), kRealPublicKey));
        assert(!cloakframe::verifyUpdateSignature(kAbcDigest, QString(), kRealPublicKey));
        assert(!cloakframe::verifyUpdateSignature(kAbcDigest, kRealSignature, QString()));
        // An empty payload is not something to sign off on either.
        assert(!cloakframe::verifyUpdateSignature(QByteArray(), kRealSignature, kRealPublicKey));
    }

    void testFileDigestMatchesTheSignedValue()
    {
        QTemporaryDir dir;
        assert(dir.isValid());
        const QString path = dir.filePath(QStringLiteral("payload.bin"));
        {
            std::ofstream out(path.toStdString(), std::ios::binary);
            out << "abc";
        }

        QString error = QStringLiteral("untouched");
        const auto digest = cloakframe::sha256HexOfFile(path, &error);
        assert(digest.has_value());
        assert(*digest == kAbcDigest);
        assert(error == QStringLiteral("untouched"));

        // The digest a file produces is exactly what the workflow signs, so the two halves
        // meet here rather than only in a release.
        assert(cloakframe::verifyUpdateSignature(*digest, kRealSignature, kRealPublicKey));
    }

    void testMissingFileReportsInsteadOfDigesting()
    {
        QTemporaryDir dir;
        assert(dir.isValid());
        QString error;
        const auto digest =
            cloakframe::sha256HexOfFile(dir.filePath(QStringLiteral("absent.bin")), &error);
        assert(!digest.has_value());
        assert(!error.isEmpty());
    }
}

int main()
{
    testRfcVectorVerifies();
    testWorkflowShapeVerifies();
    testTamperedPayloadFails();
    testOtherKeyFails();
    testTamperedSignatureFails();
    testMalformedInputIsRejected();
    testFileDigestMatchesTheSignedValue();
    testMissingFileReportsInsteadOfDigesting();
    std::puts("update signature tests passed");
    return 0;
}
