<?php

declare(strict_types=1);

namespace Fagus;

/**
 * Something the reader of an encrypted PDF may do without the owner password. Viewers enforce
 * the permissions; whoever can open the file can read its content.
 */
enum Permission: string
{
    /** Print, in full quality. */
    case Print = 'print';

    /** Print, but only as a low-resolution image; {@see Permission::Print} allows more. */
    case PrintLowQuality = 'print-low-quality';

    /** Change the content of the document. */
    case Modify = 'modify';

    /** Copy or extract text and images. */
    case Copy = 'copy';

    /** Add and change annotations (comments), and fill in form fields. */
    case Annotate = 'annotate';

    /** Fill in form fields, also without {@see Permission::Annotate}. */
    case FillForms = 'fill-forms';

    /** Insert, delete and rotate pages, and change bookmarks. */
    case Assemble = 'assemble';
}
