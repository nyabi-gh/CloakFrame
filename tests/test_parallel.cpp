#include "cloakframe/OrderedParallel.hpp"

#include <atomic>
#include <cassert>
#include <chrono>
#include <cstdio>
#include <latch>
#include <stdexcept>
#include <thread>
#include <vector>

namespace
{
    void testConsumesInOrder()
    {
        const std::size_t count = 200;
        std::atomic<bool> cancelled{false};
        std::vector<std::size_t> consumed;

        cloakframe::processOrdered<std::size_t>(
            count,
            8,
            8,
            cancelled,
            [](std::size_t index)
            {
                std::this_thread::sleep_for(std::chrono::microseconds((index * 7) % 300));
                return index * 2;
            },
            [&](std::size_t index, std::size_t &&value)
            {
                assert(value == index * 2);
                consumed.push_back(index);
            });

        assert(consumed.size() == count);
        for (std::size_t i = 0; i < count; ++i)
        {
            assert(consumed[i] == i);
        }
    }

    void testRespectsInFlightBound()
    {
        const std::size_t count = 100;
        const std::size_t maxInFlight = 3;
        std::atomic<bool> cancelled{false};
        std::atomic<int> active{0};
        std::atomic<int> peakActive{0};
        std::size_t consumedCount = 0;

        cloakframe::processOrdered<int>(
            count,
            8,
            maxInFlight,
            cancelled,
            [&](std::size_t index)
            {
                const int now = ++active;
                int peak = peakActive.load();
                while (now > peak && !peakActive.compare_exchange_weak(peak, now))
                {
                }
                std::this_thread::sleep_for(std::chrono::microseconds(200));
                --active;
                return static_cast<int>(index);
            },
            [&](std::size_t, int &&)
            {
                ++consumedCount;
            });

        assert(consumedCount == count);
        assert(peakActive.load() <= static_cast<int>(maxInFlight));
    }

    void testStopsAfterCancellation()
    {
        const std::size_t count = 10000;
        std::atomic<bool> cancelled{false};
        std::size_t consumedCount = 0;

        cloakframe::processOrdered<int>(
            count,
            4,
            4,
            cancelled,
            [](std::size_t index)
            {
                std::this_thread::sleep_for(std::chrono::microseconds(50));
                return static_cast<int>(index);
            },
            [&](std::size_t, int &&)
            {
                ++consumedCount;
                if (consumedCount == 20)
                {
                    cancelled.store(true, std::memory_order_release);
                }
            });

        assert(consumedCount >= 20);
        assert(consumedCount < count);
    }

    void testCancellationRetainsResultsFromRunningProducers()
    {
        std::atomic<bool> cancelled{false};
        std::latch started(2);
        std::vector<std::size_t> consumed;
        cloakframe::processOrdered<std::size_t>(
            10,
            2,
            2,
            cancelled,
            [&](std::size_t index)
            {
                started.count_down();
                started.wait();
                if (index == 0)
                {
                    cancelled.store(true, std::memory_order_release);
                }
                else
                {
                    std::this_thread::sleep_for(std::chrono::milliseconds(20));
                }
                return index;
            },
            [&](std::size_t index, std::size_t &&value)
            {
                assert(index == value);
                consumed.push_back(index);
            });
        assert((consumed == std::vector<std::size_t>{0, 1}));
    }

    void testSingleThreadAndEmpty()
    {
        std::atomic<bool> cancelled{false};
        std::size_t consumedCount = 0;

        cloakframe::processOrdered<int>(
            0,
            4,
            4,
            cancelled,
            [](std::size_t)
            {
                return 0;
            },
            [&](std::size_t, int &&)
            {
                ++consumedCount;
            });
        assert(consumedCount == 0);

        cloakframe::processOrdered<int>(
            5,
            1,
            1,
            cancelled,
            [](std::size_t index)
            {
                return static_cast<int>(index);
            },
            [&](std::size_t index, int &&value)
            {
                assert(static_cast<std::size_t>(value) == index);
                ++consumedCount;
            });
        assert(consumedCount == 5);
    }

    void testPropagatesProducerException()
    {
        std::atomic<bool> cancelled{false};
        bool caught = false;

        try
        {
            cloakframe::processOrdered<int>(
                200,
                8,
                8,
                cancelled,
                [](std::size_t index) -> int
                {
                    if (index == 7)
                    {
                        throw std::runtime_error("produce failed");
                    }
                    std::this_thread::sleep_for(std::chrono::microseconds(50));
                    return static_cast<int>(index);
                },
                [](std::size_t, int &&)
                {
                });
        }
        catch (const std::runtime_error &)
        {
            caught = true;
        }

        assert(caught);
    }

    void testPropagatesConsumerException()
    {
        std::atomic<bool> cancelled{false};
        bool caught = false;

        try
        {
            cloakframe::processOrdered<int>(
                200,
                8,
                8,
                cancelled,
                [](std::size_t index)
                {
                    return static_cast<int>(index);
                },
                [](std::size_t index, int &&)
                {
                    if (index == 5)
                    {
                        throw std::runtime_error("consume failed");
                    }
                });
        }
        catch (const std::runtime_error &)
        {
            caught = true;
        }

        assert(caught);
    }
}

int main()
{
    testConsumesInOrder();
    std::puts("ordered consumption: ok");
    testRespectsInFlightBound();
    std::puts("in-flight bound: ok");
    testStopsAfterCancellation();
    testCancellationRetainsResultsFromRunningProducers();
    std::puts("cancellation: ok");
    testSingleThreadAndEmpty();
    std::puts("edge cases: ok");
    testPropagatesProducerException();
    std::puts("producer exception: ok");
    testPropagatesConsumerException();
    std::puts("consumer exception: ok");
    std::puts("all parallel tests passed");
    return 0;
}
