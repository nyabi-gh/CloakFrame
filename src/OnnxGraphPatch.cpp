#include "cloakframe/OnnxGraphPatch.hpp"

#include <cstring>
#include <limits>
#include <map>
#include <set>
#include <string>

namespace cloakframe
{
    namespace
    {
        constexpr std::uint32_t kWireVarint = 0;
        constexpr std::uint32_t kWireFixed64 = 1;
        constexpr std::uint32_t kWireLengthDelimited = 2;
        constexpr std::uint32_t kWireFixed32 = 5;

        constexpr std::uint64_t kTensorInt64 = 7;

        struct PbField
        {
            std::uint32_t number = 0;
            std::uint32_t wireType = 0;
            std::uint64_t varint = 0;
            std::vector<std::uint8_t> bytes;
        };

        using PbMessage = std::vector<PbField>;

        bool readVarint(
            const std::uint8_t *data, std::size_t size, std::size_t &pos, std::uint64_t &value)
        {
            value = 0;
            for (int shift = 0; shift < 64 && pos < size; shift += 7)
            {
                const std::uint8_t byte = data[pos++];
                value |= static_cast<std::uint64_t>(byte & 0x7FU) << shift;
                if ((byte & 0x80U) == 0)
                {
                    return true;
                }
            }
            return false;
        }

        std::optional<PbMessage> parseMessage(const std::uint8_t *data, std::size_t size)
        {
            PbMessage fields;
            std::size_t pos = 0;
            const auto readNext = [&](std::uint64_t &value)
            {
                return readVarint(data, size, pos, value);
            };

            while (pos < size)
            {
                std::uint64_t key = 0;
                if (!readNext(key))
                {
                    return std::nullopt;
                }
                PbField field;
                field.number = static_cast<std::uint32_t>(key >> 3);
                field.wireType = static_cast<std::uint32_t>(key & 7U);
                if (field.number == 0)
                {
                    return std::nullopt;
                }
                switch (field.wireType)
                {
                case kWireVarint:
                    if (!readNext(field.varint))
                    {
                        return std::nullopt;
                    }
                    break;
                case kWireFixed64:
                case kWireFixed32:
                {
                    const std::size_t width = field.wireType == kWireFixed64 ? 8 : 4;
                    if (size - pos < width)
                    {
                        return std::nullopt;
                    }
                    field.bytes.assign(data + pos, data + pos + width);
                    pos += width;
                    break;
                }
                case kWireLengthDelimited:
                {
                    std::uint64_t length = 0;
                    if (!readNext(length) || length > size - pos)
                    {
                        return std::nullopt;
                    }
                    field.bytes.assign(data + pos, data + pos + length);
                    pos += length;
                    break;
                }
                default:
                    return std::nullopt;
                }
                fields.push_back(std::move(field));
            }
            return fields;
        }

        std::optional<PbMessage> parseMessage(const std::vector<std::uint8_t> &bytes)
        {
            return parseMessage(bytes.data(), bytes.size());
        }

        void appendVarint(std::vector<std::uint8_t> &out, std::uint64_t value)
        {
            while (value >= 0x80U)
            {
                out.push_back(static_cast<std::uint8_t>((value & 0x7FU) | 0x80U));
                value >>= 7;
            }
            out.push_back(static_cast<std::uint8_t>(value));
        }

        std::vector<std::uint8_t> serializeMessage(const PbMessage &fields)
        {
            std::vector<std::uint8_t> out;
            for (const auto &field : fields)
            {
                appendVarint(out, (static_cast<std::uint64_t>(field.number) << 3) | field.wireType);
                if (field.wireType == kWireVarint)
                {
                    appendVarint(out, field.varint);
                }
                else if (field.wireType == kWireLengthDelimited)
                {
                    appendVarint(out, field.bytes.size());
                    out.insert(out.end(), field.bytes.begin(), field.bytes.end());
                }
                else
                {
                    out.insert(out.end(), field.bytes.begin(), field.bytes.end());
                }
            }
            return out;
        }

        PbField makeVarintField(std::uint32_t number, std::uint64_t value)
        {
            PbField field;
            field.number = number;
            field.wireType = kWireVarint;
            field.varint = value;
            return field;
        }

        PbField *findFirst(PbMessage &fields, std::uint32_t number, std::uint32_t wireType)
        {
            for (auto &field : fields)
            {
                if (field.number == number && field.wireType == wireType)
                {
                    return &field;
                }
            }
            return nullptr;
        }

        const PbField *findFirst(
            const PbMessage &fields, std::uint32_t number, std::uint32_t wireType)
        {
            for (const auto &field : fields)
            {
                if (field.number == number && field.wireType == wireType)
                {
                    return &field;
                }
            }
            return nullptr;
        }

        std::optional<PbMessage> childMessage(const PbMessage &parent, std::uint32_t number)
        {
            const auto *field = findFirst(parent, number, kWireLengthDelimited);
            return field == nullptr ? std::nullopt : parseMessage(field->bytes);
        }

        std::string fieldString(const PbField &field)
        {
            return {field.bytes.begin(), field.bytes.end()};
        }

        bool rewriteNested(PbField &parent, std::uint32_t number, const auto &transform)
        {
            auto parsed = parseMessage(parent.bytes);
            if (!parsed)
            {
                return false;
            }
            auto *child = findFirst(*parsed, number, kWireLengthDelimited);
            if (child == nullptr || !transform(*child))
            {
                return false;
            }
            parent.bytes = serializeMessage(*parsed);
            return true;
        }

        bool setDimensionValue(PbField &dimension, int size)
        {
            const auto parsed = parseMessage(dimension.bytes);
            if (!parsed)
            {
                return false;
            }
            dimension.bytes =
                serializeMessage({makeVarintField(1, static_cast<std::uint64_t>(size))});
            return true;
        }

        bool setInputSpatialDims(PbField &input, int size)
        {
            return rewriteNested(input,
                2,
                [size](PbField &type)
                {
                    return rewriteNested(type,
                        1,
                        [size](PbField &tensor)
                        {
                            return rewriteNested(tensor,
                                2,
                                [size](PbField &shape)
                                {
                                    auto parsed = parseMessage(shape.bytes);
                                    if (!parsed)
                                    {
                                        return false;
                                    }
                                    std::vector<PbField *> dims;
                                    for (auto &field : *parsed)
                                    {
                                        if (field.number == 1
                                            && field.wireType == kWireLengthDelimited)
                                        {
                                            dims.push_back(&field);
                                        }
                                    }
                                    if (dims.size() != 4)
                                    {
                                        return false;
                                    }
                                    if (!setDimensionValue(*dims[2], size)
                                        || !setDimensionValue(*dims[3], size))
                                    {
                                        return false;
                                    }
                                    shape.bytes = serializeMessage(*parsed);
                                    return true;
                                });
                        });
                });
        }

        struct SpatialSize
        {
            std::int64_t height = 0;
            std::int64_t width = 0;
        };

        std::optional<std::int64_t> dimensionValue(const PbField &dimension)
        {
            const auto parsed = parseMessage(dimension.bytes);
            if (!parsed)
            {
                return std::nullopt;
            }
            const auto *value = findFirst(*parsed, 1, kWireVarint);
            if (value == nullptr)
            {
                return std::nullopt;
            }
            return static_cast<std::int64_t>(value->varint);
        }

        // The spatial dims the model was exported with. A dimension given as a name rather than a
        // number has no answer here, and the caller then has nothing to rescale a Resize against.
        std::optional<SpatialSize> inputSpatialDims(const PbField &input)
        {
            const auto valueInfo = parseMessage(input.bytes);
            if (!valueInfo)
            {
                return std::nullopt;
            }
            const auto type = childMessage(*valueInfo, 2);
            const auto tensor = type ? childMessage(*type, 1) : std::nullopt;
            const auto shape = tensor ? childMessage(*tensor, 2) : std::nullopt;
            if (!shape)
            {
                return std::nullopt;
            }

            std::vector<const PbField *> dims;
            for (const auto &field : *shape)
            {
                if (field.number == 1 && field.wireType == kWireLengthDelimited)
                {
                    dims.push_back(&field);
                }
            }
            if (dims.size() != 4)
            {
                return std::nullopt;
            }

            const auto height = dimensionValue(*dims[2]);
            const auto width = dimensionValue(*dims[3]);
            if (!height || !width || *height <= 0 || *width <= 0)
            {
                return std::nullopt;
            }
            return SpatialSize{*height, *width};
        }

        std::optional<std::vector<std::int64_t>> int64TensorValues(const PbMessage &tensor)
        {
            const auto *dataType = findFirst(tensor, 2, kWireVarint);
            if (dataType == nullptr || dataType->varint != kTensorInt64)
            {
                return std::nullopt;
            }

            if (const auto *raw = findFirst(tensor, 9, kWireLengthDelimited))
            {
                if (raw->bytes.size() % sizeof(std::int64_t) != 0)
                {
                    return std::nullopt;
                }
                std::vector<std::int64_t> values(raw->bytes.size() / sizeof(std::int64_t));
                std::memcpy(values.data(), raw->bytes.data(), raw->bytes.size());
                return values;
            }

            std::vector<std::int64_t> values;
            for (const auto &field : tensor)
            {
                if (field.number != 7)
                {
                    continue;
                }
                if (field.wireType == kWireVarint)
                {
                    values.push_back(static_cast<std::int64_t>(field.varint));
                }
                else if (field.wireType == kWireLengthDelimited)
                {
                    std::size_t pos = 0;
                    while (pos < field.bytes.size())
                    {
                        std::uint64_t entry = 0;
                        if (!readVarint(field.bytes.data(), field.bytes.size(), pos, entry))
                        {
                            return std::nullopt;
                        }
                        values.push_back(static_cast<std::int64_t>(entry));
                    }
                }
            }
            if (values.empty())
            {
                return std::nullopt;
            }
            return values;
        }

        // Written back as raw_data whichever of the two forms it arrived in: both are valid for an
        // INT64 tensor, and one writer covers both.
        void setInt64TensorValues(PbMessage &tensor, const std::vector<std::int64_t> &values)
        {
            std::erase_if(tensor,
                [](const PbField &field)
                {
                    return field.number == 7 || field.number == 9;
                });

            PbField raw;
            raw.number = 9;
            raw.wireType = kWireLengthDelimited;
            raw.bytes.resize(values.size() * sizeof(std::int64_t));
            std::memcpy(raw.bytes.data(), values.data(), raw.bytes.size());
            tensor.push_back(std::move(raw));
        }

        std::optional<std::int64_t> rescale(std::int64_t value, std::int64_t from, std::int64_t to)
        {
            if (value <= 0 || from <= 0 || to <= 0
                || value > std::numeric_limits<std::int64_t>::max() / to)
            {
                return std::nullopt;
            }
            const auto product = value * to;
            if (product % from != 0)
            {
                return std::nullopt;
            }
            return product / from;
        }

        // A Resize whose target size was baked in at export time still has to land on whatever the
        // rest of the graph produces at the new input size. Its spatial extent is a fixed fraction
        // of the input's, so the recorded size scales with the input and nothing about the node's
        // own input tensor has to be known.
        bool rescaleSizesInitializer(
            PbField &initializerField, const SpatialSize &original, int size)
        {
            auto tensor = parseMessage(initializerField.bytes);
            if (!tensor)
            {
                return false;
            }
            auto values = int64TensorValues(*tensor);
            if (!values || values->size() != 4)
            {
                return false;
            }

            const auto height = rescale((*values)[2], original.height, size);
            const auto width = rescale((*values)[3], original.width, size);
            if (!height || !width)
            {
                return false;
            }
            (*values)[2] = *height;
            (*values)[3] = *width;

            setInt64TensorValues(*tensor, *values);
            initializerField.bytes = serializeMessage(*tensor);
            return true;
        }

        bool makeOutputShapeUnknown(PbField &output)
        {
            return rewriteNested(output,
                2,
                [](PbField &type)
                {
                    return rewriteNested(type,
                        1,
                        [](PbField &tensor)
                        {
                            auto parsed = parseMessage(tensor.bytes);
                            if (!parsed)
                            {
                                return false;
                            }
                            auto *shape = findFirst(*parsed, 2, kWireLengthDelimited);
                            if (shape == nullptr)
                            {
                                return true;
                            }
                            auto shapeFields = parseMessage(shape->bytes);
                            if (!shapeFields)
                            {
                                return false;
                            }
                            for (auto &dimension : *shapeFields)
                            {
                                if (dimension.number == 1
                                    && dimension.wireType == kWireLengthDelimited)
                                {
                                    dimension.bytes.clear();
                                }
                            }
                            shape->bytes = serializeMessage(*shapeFields);
                            tensor.bytes = serializeMessage(*parsed);
                            return true;
                        });
                });
        }

        bool resizeModeIsSupported(const PbMessage &node)
        {
            for (const auto &field : node)
            {
                if (field.number != 5 || field.wireType != kWireLengthDelimited)
                {
                    continue;
                }
                const auto attribute = parseMessage(field.bytes);
                if (!attribute)
                {
                    return false;
                }
                std::string name;
                std::string text;
                for (const auto &entry : *attribute)
                {
                    if (entry.number == 1 && entry.wireType == kWireLengthDelimited)
                    {
                        name = fieldString(entry);
                    }
                    else if (entry.number == 4 && entry.wireType == kWireLengthDelimited)
                    {
                        text = fieldString(entry);
                    }
                }
                if (name == "mode" && text != "nearest")
                {
                    return false;
                }
                if (name == "coordinate_transformation_mode" && text != "asymmetric")
                {
                    return false;
                }
            }
            return true;
        }

        enum class ResizeScan
        {
            NotResize,
            Rescalable,
            Unsupported,
        };

        // Records which initializer holds this Resize's target size. The node is not touched:
        // rewriting that initializer is what moves the Resize to the new input size.
        ResizeScan scanResizeNode(const PbField &nodeField,
            const std::set<std::string> &initializerNames,
            std::map<std::string, std::size_t> &sizesUses)
        {
            const auto node = parseMessage(nodeField.bytes);
            if (!node)
            {
                return ResizeScan::Unsupported;
            }
            const auto *opType = findFirst(*node, 4, kWireLengthDelimited);
            if (opType == nullptr || fieldString(*opType) != "Resize")
            {
                return ResizeScan::NotResize;
            }

            std::vector<const PbField *> inputs;
            for (const auto &field : *node)
            {
                if (field.number == 1 && field.wireType == kWireLengthDelimited)
                {
                    inputs.push_back(&field);
                }
            }
            if (inputs.size() != 4)
            {
                return ResizeScan::NotResize;
            }
            const auto sizesName = fieldString(*inputs[3]);
            if (!initializerNames.contains(sizesName))
            {
                return ResizeScan::NotResize;
            }
            if (!resizeModeIsSupported(*node))
            {
                return ResizeScan::Unsupported;
            }

            ++sizesUses[sizesName];
            return ResizeScan::Rescalable;
        }
    }

    std::optional<std::vector<std::uint8_t>> makeOnnxSpatialDimsFixed(
        const std::vector<std::uint8_t> &modelBytes, int size)
    {
        if (size <= 0)
        {
            return std::nullopt;
        }
        auto model = parseMessage(modelBytes);
        if (!model)
        {
            return std::nullopt;
        }
        auto *graphField = findFirst(*model, 7, kWireLengthDelimited);
        if (graphField == nullptr)
        {
            return std::nullopt;
        }
        auto graph = parseMessage(graphField->bytes);
        if (!graph)
        {
            return std::nullopt;
        }

        std::set<std::string> initializerNames;
        for (auto &field : *graph)
        {
            if (field.number != 5 || field.wireType != kWireLengthDelimited)
            {
                continue;
            }
            auto initializer = parseMessage(field.bytes);
            if (!initializer)
            {
                return std::nullopt;
            }
            if (const auto *name = findFirst(*initializer, 8, kWireLengthDelimited))
            {
                initializerNames.insert(fieldString(*name));
            }
        }
        std::vector<PbField *> inputs;
        for (auto &field : *graph)
        {
            if (field.number == 11 && field.wireType == kWireLengthDelimited)
            {
                inputs.push_back(&field);
            }
        }
        if (inputs.size() != 1)
        {
            return std::nullopt;
        }
        const auto exportedSize = inputSpatialDims(*inputs.front());
        if (!setInputSpatialDims(*inputs.front(), size))
        {
            return std::nullopt;
        }

        std::map<std::string, std::size_t> sizesUses;
        for (const auto &field : *graph)
        {
            if (field.number == 1 && field.wireType == kWireLengthDelimited
                && scanResizeNode(field, initializerNames, sizesUses) == ResizeScan::Unsupported)
            {
                return std::nullopt;
            }
        }

        for (auto &field : *graph)
        {
            if (field.number == 12 && field.wireType == kWireLengthDelimited)
            {
                if (!makeOutputShapeUnknown(field))
                {
                    return std::nullopt;
                }
            }
        }

        std::erase_if(*graph,
            [](const PbField &field)
            {
                return field.number == 13;
            });

        if (!sizesUses.empty())
        {
            if (!exportedSize)
            {
                return std::nullopt;
            }

            // Rewriting a constant in place is only correct while the Resize is its only reader,
            // and an exporter is free to share one constant between unrelated nodes.
            std::map<std::string, std::size_t> allUses;
            for (const auto &field : *graph)
            {
                if (field.number != 1 || field.wireType != kWireLengthDelimited)
                {
                    continue;
                }
                const auto node = parseMessage(field.bytes);
                if (!node)
                {
                    return std::nullopt;
                }
                for (const auto &entry : *node)
                {
                    if (entry.number == 1 && entry.wireType == kWireLengthDelimited)
                    {
                        ++allUses[fieldString(entry)];
                    }
                }
            }
            for (const auto &[name, uses] : sizesUses)
            {
                if (allUses[name] != uses)
                {
                    return std::nullopt;
                }
            }

            for (auto &field : *graph)
            {
                if (field.number != 5 || field.wireType != kWireLengthDelimited)
                {
                    continue;
                }
                const auto initializer = parseMessage(field.bytes);
                if (!initializer)
                {
                    return std::nullopt;
                }
                const auto *name = findFirst(*initializer, 8, kWireLengthDelimited);
                if (name == nullptr || !sizesUses.contains(fieldString(*name)))
                {
                    continue;
                }
                if (!rescaleSizesInitializer(field, *exportedSize, size))
                {
                    return std::nullopt;
                }
            }
        }

        graphField->bytes = serializeMessage(*graph);
        return serializeMessage(*model);
    }
}
