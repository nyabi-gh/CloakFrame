#include "cloakframe/UpdateSignature.hpp"

#include <QByteArray>
#include <QString>
#include <QTemporaryDir>

#include <cassert>
#include <cstdio>
#include <fstream>

namespace
{
    // These are functions rather than namespace-scope constants because constructing a QString
    // at static initialisation can throw where nothing is able to catch it.

    // RFC 8032 section 7.1, test vector 2: a one-byte message, so the vector proves the
    // verification itself rather than anything about how CloakFrame frames a payload.
    QString rfcPublicKey()
    {
        return QStringLiteral("PUAXw+hDiVqStwqnTRt+vJyYLM8uxJaMwM1V8Sr0Zgw=");
    }

    QString rfcSignature()
    {
        return QStringLiteral(
            "kqAJqfDUyrhyDoILX2QlQKKye1QWUD+Ps3YiI+vbadoIWsHkPhWZbkWPNhPQ8R2MOHsurrQwKu6wDSkWErsM"
            "AA==");
    }

    QByteArray rfcMessage()
    {
        return QByteArray::fromHex("72");
    }

    // The shape the release workflow actually produces: an Ed25519 signature over the lowercase
    // hex SHA-256 of a file, made with `openssl pkeyutl -sign -rawin`. The digest is of "abc".
    QString realPublicKey()
    {
        return QStringLiteral("b8LBqH0YWEKwE6iZJ+gqey5I13A/rT8srBWBUSrICPQ=");
    }

    QString realSignature()
    {
        return QStringLiteral(
            "z1TZzXjZnAH2z9T5z7Ul1wlN5SrLos/FjdN87mVX6Vc0hCI1V7q82NIdhNEBjaupJAsDVVN0bzgULbVZR5hN"
            "AQ==");
    }

    QByteArray abcDigest()
    {
        return QByteArrayLiteral(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    void testRfcVectorVerifies()
    {
        QString error = QStringLiteral("untouched");
        assert(cloakframe::verifyUpdateSignature(
            rfcMessage(), rfcSignature(), rfcPublicKey(), &error));
        assert(error == QStringLiteral("untouched"));
    }

    void testWorkflowShapeVerifies()
    {
        assert(cloakframe::verifyUpdateSignature(abcDigest(), realSignature(), realPublicKey()));
    }

    void testTamperedPayloadFails()
    {
        QByteArray tampered = abcDigest();
        tampered[0] = 'c';
        QString error;
        assert(
            !cloakframe::verifyUpdateSignature(tampered, realSignature(), realPublicKey(), &error));
        assert(!error.isEmpty());
    }

    void testOtherKeyFails()
    {
        // A signature that verifies under its own key must not verify under another real key.
        assert(!cloakframe::verifyUpdateSignature(abcDigest(), realSignature(), rfcPublicKey()));
        assert(!cloakframe::verifyUpdateSignature(rfcMessage(), rfcSignature(), realPublicKey()));
    }

    void testTamperedSignatureFails()
    {
        QByteArray raw = QByteArray::fromBase64(realSignature().toUtf8());
        raw[0] = static_cast<char>(raw[0] ^ 0x01);
        assert(!cloakframe::verifyUpdateSignature(
            abcDigest(), QString::fromUtf8(raw.toBase64()), realPublicKey()));
    }

    void testMalformedInputIsRejected()
    {
        // Every one of these has to fail closed: a build that cannot parse what it was given
        // knows nothing about the update, which is not the same as the update being genuine.
        QString error;
        assert(!cloakframe::verifyUpdateSignature(
            abcDigest(), QStringLiteral("not base64!!"), realPublicKey(), &error));
        assert(!error.isEmpty());

        assert(!cloakframe::verifyUpdateSignature(
            abcDigest(), realSignature(), QStringLiteral("not base64!!")));
        // Right encoding, wrong length.
        assert(!cloakframe::verifyUpdateSignature(
            abcDigest(), realSignature(), QStringLiteral("YWJj")));
        assert(!cloakframe::verifyUpdateSignature(
            abcDigest(), QStringLiteral("YWJj"), realPublicKey()));
        assert(!cloakframe::verifyUpdateSignature(abcDigest(), QString(), realPublicKey()));
        assert(!cloakframe::verifyUpdateSignature(abcDigest(), realSignature(), QString()));
        // An empty payload is not something to sign off on either.
        assert(!cloakframe::verifyUpdateSignature(QByteArray(), realSignature(), realPublicKey()));
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
        assert(*digest == abcDigest());
        assert(error == QStringLiteral("untouched"));

        // The digest a file produces is exactly what the workflow signs, so the two halves
        // meet here rather than only in a release.
        assert(cloakframe::verifyUpdateSignature(*digest, realSignature(), realPublicKey()));
    }

    void testTrustFollowsTheSignatureOverTheDeclaredDigest()
    {
        QString error = QStringLiteral("untouched");
        assert(cloakframe::evaluateUpdateTrust(
                   QString::fromUtf8(abcDigest()), realSignature(), realPublicKey(), &error)
               == cloakframe::UpdateTrust::Trusted);
        assert(error == QStringLiteral("untouched"));

        // The signature is made over the lowercase form, so a feed that shouts must still work.
        assert(cloakframe::evaluateUpdateTrust(
                   QString::fromUtf8(abcDigest()).toUpper(), realSignature(), realPublicKey())
               == cloakframe::UpdateTrust::Trusted);
        assert(cloakframe::evaluateUpdateTrust(
                   QStringLiteral("  %1  ").arg(QString::fromUtf8(abcDigest())),
                   realSignature(),
                   realPublicKey())
               == cloakframe::UpdateTrust::Trusted);
    }

    void testTrustIsRefusedForAnythingUnproven()
    {
        // A different digest than the one signed: the package the feed points at is not the
        // package the key holder approved.
        QString other = QString::fromUtf8(abcDigest());
        other[0] = QLatin1Char('c');
        assert(cloakframe::evaluateUpdateTrust(other, realSignature(), realPublicKey())
               == cloakframe::UpdateTrust::Rejected);

        QString error;
        assert(cloakframe::evaluateUpdateTrust(
                   QStringLiteral("not-a-digest"), realSignature(), realPublicKey(), &error)
               == cloakframe::UpdateTrust::Rejected);
        assert(!error.isEmpty());
        // Right length, wrong alphabet.
        assert(cloakframe::evaluateUpdateTrust(
                   QString(64, QLatin1Char('z')), realSignature(), realPublicKey())
               == cloakframe::UpdateTrust::Rejected);
        assert(cloakframe::evaluateUpdateTrust(
                   QString::fromUtf8(abcDigest()), QString(), realPublicKey())
               == cloakframe::UpdateTrust::Rejected);
        assert(cloakframe::evaluateUpdateTrust(
                   QString::fromUtf8(abcDigest()), realSignature(), rfcPublicKey())
               == cloakframe::UpdateTrust::Rejected);
    }

    void testAnUnpinnedBuildSaysSoInsteadOfPassing()
    {
        // Distinct from Trusted on purpose: the caller has to decide what an unpinned build
        // does, and cannot mistake "nothing to check" for "checked and good".
        assert(cloakframe::evaluateUpdateTrust(
                   QString::fromUtf8(abcDigest()), realSignature(), QString())
               == cloakframe::UpdateTrust::Unpinned);
        assert(cloakframe::evaluateUpdateTrust(QStringLiteral("nonsense"), QString(), QString())
               == cloakframe::UpdateTrust::Unpinned);
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
    testTrustFollowsTheSignatureOverTheDeclaredDigest();
    testTrustIsRefusedForAnythingUnproven();
    testAnUnpinnedBuildSaysSoInsteadOfPassing();
    testMissingFileReportsInsteadOfDigesting();
    std::puts("update signature tests passed");
    return 0;
}
