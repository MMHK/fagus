<?php

declare(strict_types=1);

namespace Fagus;

/**
 * What a certified PDF still allows after its certification signature (DocMDP): changes beyond
 * them make readers report the certification as broken. Validation data and document timestamps
 * may always be added.
 */
enum Certification: string
{
    /** No changes. */
    case NoChanges = 'no-changes';

    /** Filling in forms and signing. */
    case FillForms = 'fill-forms';

    /** Filling in forms, signing and adding or changing comments. */
    case Annotate = 'annotate';
}
