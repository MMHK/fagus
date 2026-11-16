<?php

declare(strict_types=1);

namespace Fagus;

/**
 * How an attached file relates to the document (PDF 2.0 `AFRelationship`).
 */
enum Relationship: string
{
    case Source = 'source';
    case Data = 'data';
    case Alternative = 'alternative';
    case Supplement = 'supplement';
    case Unspecified = 'unspecified';
}
