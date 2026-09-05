#pragma once

#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <cstddef>
#include <exception>
#include <map>
#include <mutex>
#include <thread>
#include <vector>

namespace cloakframe
{
    template <typename Result, typename Produce, typename Consume>
    void processOrdered(std::size_t itemCount,
        unsigned threadCount,
        std::size_t maxInFlight,
        const std::atomic<bool> &cancelled,
        Produce produce,
        Consume consume)
    {
        if (itemCount == 0)
        {
            return;
        }
        threadCount = std::max(1U, threadCount);
        maxInFlight = std::max<std::size_t>(1, maxInFlight);

        std::mutex mutex;
        std::condition_variable produceCv;
        std::condition_variable consumeCv;
        std::size_t nextIndex = 0;
        std::size_t consumedCount = 0;
        std::map<std::size_t, Result> ready;
        bool stopped = false;
        std::exception_ptr workerError;

        auto worker = [&]
        {
            try
            {
                for (;;)
                {
                    std::size_t index = 0;
                    {
                        std::unique_lock lock(mutex);
                        produceCv.wait(lock,
                            [&]
                            {
                                return stopped || nextIndex >= itemCount
                                       || nextIndex < consumedCount + maxInFlight;
                            });
                        if (stopped || nextIndex >= itemCount)
                        {
                            return;
                        }
                        index = nextIndex++;
                    }
                    if (cancelled.load(std::memory_order_acquire))
                    {
                        std::lock_guard lock(mutex);
                        stopped = true;
                        produceCv.notify_all();
                        consumeCv.notify_all();
                        return;
                    }
                    Result result = produce(index);
                    {
                        std::lock_guard lock(mutex);
                        ready.emplace(index, std::move(result));
                        consumeCv.notify_all();
                    }
                }
            }
            catch (...)
            {
                std::lock_guard lock(mutex);
                if (!workerError)
                {
                    workerError = std::current_exception();
                }
                stopped = true;
                produceCv.notify_all();
                consumeCv.notify_all();
            }
        };

        std::vector<std::thread> workers;

        // Destroying a joinable std::thread calls std::terminate, so every exit path -
        // including a throwing thread constructor or a throwing consume() - must stop and
        // join whatever was already started before `workers` is destroyed.
        struct StopAndJoin
        {
            std::vector<std::thread> &workers;
            std::mutex &mutex;
            std::condition_variable &produceCv;
            std::condition_variable &consumeCv;
            bool &stopped;

            ~StopAndJoin()
            {
                {
                    std::lock_guard lock(mutex);
                    stopped = true;
                }
                produceCv.notify_all();
                consumeCv.notify_all();
                for (auto &thread : workers)
                {
                    if (thread.joinable())
                    {
                        thread.join();
                    }
                }
            }
        } stopAndJoin{workers, mutex, produceCv, consumeCv, stopped};

        workers.reserve(threadCount);
        for (unsigned i = 0; i < threadCount; ++i)
        {
            workers.emplace_back(worker);
        }

        {
            std::unique_lock lock(mutex);
            while (consumedCount < itemCount)
            {
                consumeCv.wait(lock,
                    [&]
                    {
                        return stopped || ready.contains(consumedCount);
                    });
                const auto it = ready.find(consumedCount);
                if (it == ready.end())
                {
                    break;
                }
                auto node = ready.extract(it);
                lock.unlock();
                consume(consumedCount, std::move(node.mapped()));
                lock.lock();
                ++consumedCount;
                produceCv.notify_all();
            }
            stopped = true;
            produceCv.notify_all();
        }

        for (auto &thread : workers)
        {
            if (thread.joinable())
            {
                thread.join();
            }
        }

        if (workerError)
        {
            std::rethrow_exception(workerError);
        }

        // Cancellation stops admission, but an already running producer may have published
        // its output. Deliver its result after joining so it cannot disappear from the report.
        for (auto &[index, result] : ready)
        {
            consume(index, std::move(result));
        }
    }
}
