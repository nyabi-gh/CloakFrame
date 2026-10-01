#include "cloakframe/CustomModelConsent.hpp"
#include "cloakframe/ModelCatalog.hpp"
#include "cloakframe/ModelDigest.hpp"

#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QString>
#include <QTemporaryDir>

#include <atomic>
#include <cassert>
#include <cstdio>
#include <fstream>

namespace
{
    void write(const QString &path, const std::string &bytes)
    {
        std::ofstream out(path.toStdString(), std::ios::binary | std::ios::trunc);
        out << bytes;
    }

    cloakframe::CustomModelApproval approvalFor(const QString &path)
    {
        const auto digest = cloakframe::digestModelFile(path);
        return cloakframe::approvalForDigest(digest.sha256, digest.size);
    }

    bool covers(const cloakframe::CustomModelApproval &approval, const QString &path)
    {
        const auto digest = cloakframe::digestModelFile(path);
        return cloakframe::approvalCovers(approval, digest.sha256, digest.size);
    }

    void testAnApprovalRecordsTheContent()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = approvalFor(path);
        assert(approval.isRecorded());
        assert(approval.size == 10);
        assert(approval.digest.size() == 64);
        assert(approval.digest == approval.digest.toLower());

        assert(!approvalFor(root.filePath(QStringLiteral("absent.onnx"))).isRecorded());
        assert(!approvalFor(root.path()).isRecorded());
    }

    void testTheApprovedBytesAreCovered()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        assert(covers(approvalFor(path), path));
    }

    void testContentOfTheSameLengthIsStillNoticed()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = approvalFor(path);

        // Same size, different bytes. This is the case a size check alone would wave through,
        // and the one a replacement would be built to look like.
        write(path, "ONNX BYTES");
        assert(approval.size == QFileInfo(path).size());
        assert(!covers(approval, path));
    }

    void testADifferentLengthIsNoticed()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = approvalFor(path);

        write(path, "onnx bytes and then some");
        assert(!covers(approval, path));
    }

    void testAnUnrecordedApprovalApprovesNothing()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        // What a settings file written before approvals were content-bound restores. It must
        // mean "ask again", not "anything at this path is fine".
        const cloakframe::CustomModelApproval empty;
        assert(!empty.isRecorded());
        assert(!covers(empty, path));

        cloakframe::CustomModelApproval sizeOnly;
        sizeOnly.size = 10;
        assert(!sizeOnly.isRecorded());
        assert(!covers(sizeOnly, path));
    }

    void testTheDigestIsTheSha256OfTheBytes()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto digest = cloakframe::digestModelFile(path);
        assert(digest.isValid());
        assert(digest.canonicalPath == QFileInfo(path).canonicalFilePath());
        assert(digest.size == 10);
        assert(digest.sha256 == QCryptographicHash::hash("onnx bytes", QCryptographicHash::Sha256));

        assert(!cloakframe::approvalForDigest(QByteArray(31, 'x'), digest.size).isRecorded());
        const auto approval = cloakframe::approvalForDigest(digest.sha256, digest.size);
        assert(!cloakframe::approvalCovers(approval, digest.sha256.left(31), digest.size));
    }

    void testFilesOutsideTheLimitsGetNoDigest()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString empty = root.filePath(QStringLiteral("empty.onnx"));
        write(empty, "");
        assert(!cloakframe::digestModelFile(empty).isValid());

        const QString large = root.filePath(QStringLiteral("large.onnx"));
        QFile file(large);
        assert(file.open(QIODevice::WriteOnly));
        assert(file.resize(cloakframe::kMaxCustomModelBytes + 1));
        file.close();
        assert(!cloakframe::digestModelFile(large).isValid());

        assert(!cloakframe::digestModelFile(QString()).isValid());
    }

    void testACancelledDigestIsEmpty()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const std::atomic_bool cancel{true};
        assert(!cloakframe::digestModelFile(path, &cancel).isValid());
    }

#ifndef _WIN32
    void testRepointingASymlinkIsNoticed()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString approved = root.filePath(QStringLiteral("approved.onnx"));
        const QString other = root.filePath(QStringLiteral("other.onnx"));
        write(approved, "onnx bytes");
        write(other, "OTHER BYTES");

        const QString link = root.filePath(QStringLiteral("model.onnx"));
        assert(QFile::link(approved, link));

        const auto approval = approvalFor(link);
        assert(covers(approval, link));

        // The path the user approved still resolves, and to a readable ONNX file. What changed
        // is which one.
        assert(QFile::remove(link));
        assert(QFile::link(other, link));
        assert(!covers(approval, link));
    }
#endif
}

int main()
{
    testAnApprovalRecordsTheContent();
    testTheApprovedBytesAreCovered();
    testContentOfTheSameLengthIsStillNoticed();
    testADifferentLengthIsNoticed();
    testAnUnrecordedApprovalApprovesNothing();
    testTheDigestIsTheSha256OfTheBytes();
    testFilesOutsideTheLimitsGetNoDigest();
    testACancelledDigestIsEmpty();
#ifndef _WIN32
    testRepointingASymlinkIsNoticed();
#endif
    std::puts("custom model consent tests passed");
    return 0;
}
