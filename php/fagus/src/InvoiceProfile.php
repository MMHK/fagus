<?php

declare(strict_types=1);

namespace Fagus;

/**
 * The profile of a ZUGFeRD or Factur-X invoice.
 */
enum InvoiceProfile: string
{
    case Minimum = 'minimum';
    case BasicWl = 'basicwl';
    case Basic = 'basic';
    case En16931 = 'en16931';
    case Extended = 'extended';
    case XRechnung = 'xrechnung';
}
