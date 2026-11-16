<!doctype html>
<html lang="de">
<head><style>body { font-family: Inter } h1 { color: #1d4ed8 }</style></head>
<body>
<h1>Rechnung {{ $number }}</h1>
@foreach ($lines as $line)
    <p>{{ $line }}</p>
@endforeach
</body>
</html>
