#include "cloakframe/CustomModelConsent.hpp"

#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QString>
#include <QTemporaryDir>

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

    void testAnApprovalRecordsTheContent()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = cloakframe::approvalForCustomModel(path);
        assert(approval);
        assert(approval->isRecorded());
        assert(approval->size == 10);
        assert(approval->digest.size() == 64);
        assert(approval->digest == approval->digest.toLower());

        assert(!cloakframe::approvalForCustomModel(root.filePath(QStringLiteral("absent.onnx"))));
        assert(!cloakframe::approvalForCustomModel(root.path()));
    }

    struct Content
    {
        QByteArray sha256;
        qint64 size = 0;
    };

    // What the run start computes for the detector: the raw digest and the byte count.
    Content contentOf(const QString &path)
    {
        QFile file(path);
        assert(file.open(QIODevice::ReadOnly));
        const QByteArray bytes = file.readAll();
        return {QCryptographicHash::hash(bytes, QCryptographicHash::Sha256), bytes.size()};
    }

    bool covers(const cloakframe::CustomModelApproval &approval, const QString &path)
    {
        const auto content = contentOf(path);
        return cloakframe::approvalCovers(approval, content.sha256, content.size);
    }

    void testTheApprovedBytesAreCovered()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = cloakframe::approvalForCustomModel(path);
        assert(approval);
        assert(covers(*approval, path));
    }

    void testContentOfTheSameLengthIsStillNoticed()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = cloakframe::approvalForCustomModel(path);
        assert(approval);

        // Same size, different bytes. This is the case a size check alone would wave through,
        // and the one a replacement would be built to look like.
        write(path, "ONNX BYTES");
        assert(approval->size == QFileInfo(path).size());
        assert(!covers(*approval, path));
    }

    void testADifferentLengthIsNoticed()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = cloakframe::approvalForCustomModel(path);
        assert(approval);

        write(path, "onnx bytes and then some");
        assert(!covers(*approval, path));
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

    void testAnApprovalFromADigestMatchesOneFromTheFile()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto content = contentOf(path);
        const auto fromDigest = cloakframe::approvalForDigest(content.sha256, content.size);
        assert(fromDigest.isRecorded());
        assert(fromDigest == *cloakframe::approvalForCustomModel(path));
        assert(!cloakframe::approvalForDigest(QByteArray(31, 'x'), content.size).isRecorded());
        assert(!cloakframe::approvalCovers(fromDigest, content.sha256.left(31), content.size));
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

        const auto approval = cloakframe::approvalForCustomModel(link);
        assert(approval);
        assert(covers(*approval, link));

        // The path the user approved still resolves, and to a readable ONNX file. What changed
        // is which one.
        assert(QFile::remove(link));
        assert(QFile::link(other, link));
        assert(!covers(*approval, link));
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
    testAnApprovalFromADigestMatchesOneFromTheFile();
#ifndef _WIN32
    testRepointingASymlinkIsNoticed();
#endif
    std::puts("custom model consent tests passed");
    return 0;
}
