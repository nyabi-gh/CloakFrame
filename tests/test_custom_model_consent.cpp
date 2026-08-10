#include "cloakframe/CustomModelConsent.hpp"

#include <QDir>
#include <QFile>
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

    void testTheApprovedFileIsAccepted()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = cloakframe::approvalForCustomModel(path);
        assert(approval);
        assert(cloakframe::checkCustomModel(path, *approval)
               == cloakframe::CustomModelState::Approved);
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
        assert(cloakframe::checkCustomModel(path, *approval)
               == cloakframe::CustomModelState::Unapproved);
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
        assert(cloakframe::checkCustomModel(path, *approval)
               == cloakframe::CustomModelState::Unapproved);
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
        assert(
            cloakframe::checkCustomModel(path, empty) == cloakframe::CustomModelState::Unapproved);

        cloakframe::CustomModelApproval sizeOnly;
        sizeOnly.size = 10;
        assert(!sizeOnly.isRecorded());
        assert(cloakframe::checkCustomModel(path, sizeOnly)
               == cloakframe::CustomModelState::Unapproved);
    }

    void testAMissingFileIsNotADenial()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("model.onnx"));
        write(path, "onnx bytes");

        const auto approval = cloakframe::approvalForCustomModel(path);
        assert(approval);

        assert(QFile::remove(path));
        assert(cloakframe::checkCustomModel(path, *approval)
               == cloakframe::CustomModelState::Unavailable);
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
        assert(cloakframe::checkCustomModel(link, *approval)
               == cloakframe::CustomModelState::Approved);

        // The path the user approved still resolves, and to a readable ONNX file. What changed
        // is which one.
        assert(QFile::remove(link));
        assert(QFile::link(other, link));
        assert(cloakframe::checkCustomModel(link, *approval)
               == cloakframe::CustomModelState::Unapproved);
    }
#endif
}

int main()
{
    testAnApprovalRecordsTheContent();
    testTheApprovedFileIsAccepted();
    testContentOfTheSameLengthIsStillNoticed();
    testADifferentLengthIsNoticed();
    testAnUnrecordedApprovalApprovesNothing();
    testAMissingFileIsNotADenial();
#ifndef _WIN32
    testRepointingASymlinkIsNoticed();
#endif
    std::puts("custom model consent tests passed");
    return 0;
}
