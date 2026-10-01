// Linked into every test executable. On Windows a failed assert or an abort otherwise opens a
// dialog and waits for a click, so a failing test hangs until its timeout instead of failing.
#ifdef _WIN32
#include <Windows.h>
#include <crtdbg.h>
#include <cstdlib>

namespace
{
    struct FailuresGoToStderr
    {
        FailuresGoToStderr() noexcept
        {
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
            _set_error_mode(_OUT_TO_STDERR);
            _set_abort_behavior(0, _WRITE_ABORT_MSG | _CALL_REPORTFAULT);
            _CrtSetReportMode(_CRT_ASSERT, _CRTDBG_MODE_FILE);
            _CrtSetReportFile(_CRT_ASSERT, _CRTDBG_FILE_STDERR);
            _CrtSetReportMode(_CRT_ERROR, _CRTDBG_MODE_FILE);
            _CrtSetReportFile(_CRT_ERROR, _CRTDBG_FILE_STDERR);
        }
    };

    const FailuresGoToStderr kFailuresGoToStderr;
}
#endif
