#include "cloakframe/UpdateSignature.hpp"

#include <QByteArray>
#include <QString>
#include <QTemporaryDir>

#include <cassert>
#include <cstdio>
#include <fstream>
#include <initializer_list>

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

    // The digest-only shape clients up to 1.11.3 check: an Ed25519 signature over the lowercase
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

    // Made by sign_update_packages.sh with a throwaway key for channel "win", version 1.12.0
    // and a package named CloakFrame-1.12.0-full.nupkg whose content is "abc".
    QString releasePublicKey()
    {
        return QStringLiteral("pj5PD2/aCU//cagRcHnlYKJAdW9xSr9q1b2Gm2pzs24=");
    }

    QString releaseSignature()
    {
        return QStringLiteral(
            "ODzLa/zVr09xJ6rG8Sbn5AoWLSAAolzKuSn0PZsRJYLMma88Xm0gOUXdilSU9aLw8ItUxlRHRxlW2fH+5u8+"
            "DA==");
    }

    QString releaseLegacySignature()
    {
        return QStringLiteral(
            "LVwktqqR85Zaned4zCaMRCq1OuT5yOyELtqqFLdnqKpqV9Bd7v+yhsNsiKnL/zb16G94LGdeAmt/mnFYZHzy"
            "DA==");
    }

    cloakframe::UpdateRelease signedRelease()
    {
        return {QStringLiteral("win"),
            QStringLiteral("1.12.0"),
            QStringLiteral("CloakFrame-1.12.0-full.nupkg"),
            QString::fromUtf8(abcDigest())};
    }

    void testTrustFollowsTheSignatureOverTheDeclaredRelease()
    {
        QString error = QStringLiteral("untouched");
        assert(cloakframe::evaluateUpdateTrust(
                   signedRelease(), releaseSignature(), releasePublicKey(), &error)
               == cloakframe::UpdateTrust::Trusted);
        assert(error == QStringLiteral("untouched"));

        // The signature is made over the lowercase form, so a feed that shouts must still work.
        auto shouting = signedRelease();
        shouting.sha256Hex = QStringLiteral("  %1  ").arg(shouting.sha256Hex.toUpper());
        assert(cloakframe::evaluateUpdateTrust(shouting, releaseSignature(), releasePublicKey())
               == cloakframe::UpdateTrust::Trusted);

        // The same run also writes the digest-only signature that clients up to 1.11.3 check.
        assert(cloakframe::verifyUpdateSignature(
            abcDigest(), releaseLegacySignature(), releasePublicKey()));
    }

    void testTrustIsRefusedForAnythingUnproven()
    {
        // An older signed package republished as a newer version, under another channel or file
        // name, or with another digest: none of it is what the key holder approved.
        for (const auto &change :
            std::initializer_list<void (*)(cloakframe::UpdateRelease &)>{
                [](cloakframe::UpdateRelease &r)
                {
                    r.version = QStringLiteral("1.13.0");
                },
                [](cloakframe::UpdateRelease &r)
                {
                    r.channel = QStringLiteral("linux");
                },
                [](cloakframe::UpdateRelease &r)
                {
                    r.fileName = QStringLiteral("CloakFrame-1.13.0-full.nupkg");
                },
                [](cloakframe::UpdateRelease &r)
                {
                    r.sha256Hex[0] = QLatin1Char('c');
                }})
        {
            auto release = signedRelease();
            change(release);
            assert(cloakframe::evaluateUpdateTrust(release, releaseSignature(), releasePublicKey())
                   == cloakframe::UpdateTrust::Rejected);
        }

        // A digest-only signature is not accepted in place of one over the release.
        assert(cloakframe::evaluateUpdateTrust(
                   signedRelease(), releaseLegacySignature(), releasePublicKey())
               == cloakframe::UpdateTrust::Rejected);

        QString error;
        auto malformed = signedRelease();
        malformed.sha256Hex = QStringLiteral("not-a-digest");
        assert(cloakframe::evaluateUpdateTrust(
                   malformed, releaseSignature(), releasePublicKey(), &error)
               == cloakframe::UpdateTrust::Rejected);
        assert(!error.isEmpty());
        malformed.sha256Hex = QString(64, QLatin1Char('z'));
        assert(cloakframe::evaluateUpdateTrust(malformed, releaseSignature(), releasePublicKey())
               == cloakframe::UpdateTrust::Rejected);

        // A line break would let one field pass for another in the signed text.
        auto injected = signedRelease();
        injected.version = QStringLiteral("1.12.0\nchannel: win");
        assert(!cloakframe::updateSignaturePayload(injected));
        injected = signedRelease();
        injected.fileName.clear();
        assert(!cloakframe::updateSignaturePayload(injected));

        assert(cloakframe::evaluateUpdateTrust(signedRelease(), QString(), releasePublicKey())
               == cloakframe::UpdateTrust::Rejected);
        assert(cloakframe::evaluateUpdateTrust(signedRelease(), releaseSignature(), rfcPublicKey())
               == cloakframe::UpdateTrust::Rejected);
    }

    void testAnUnpinnedBuildSaysSoInsteadOfPassing()
    {
        // Distinct from Trusted on purpose: the caller has to decide what an unpinned build
        // does, and cannot mistake "nothing to check" for "checked and good".
        assert(cloakframe::evaluateUpdateTrust(signedRelease(), releaseSignature(), QString())
               == cloakframe::UpdateTrust::Unpinned);
        assert(cloakframe::evaluateUpdateTrust({}, QString(), QString())
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
    testTrustFollowsTheSignatureOverTheDeclaredRelease();
    testTrustIsRefusedForAnythingUnproven();
    testAnUnpinnedBuildSaysSoInsteadOfPassing();
    testMissingFileReportsInsteadOfDigesting();
    std::puts("update signature tests passed");
    return 0;
}
